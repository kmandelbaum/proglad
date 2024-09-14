use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use web_sys::{
    window, Document, HtmlElement, HtmlInputElement, Response, ScrollBehavior,
    ScrollIntoViewOptions, ScrollLogicalPosition, SvgElement,
};

use proglad_api::textapi;
use proglad_api::visualize::*;

mod svg;

// Engine

#[derive(Debug)]
struct VisibleObject {
    z_index: u8,
    geometry: Vec<geom::Geom>,
    base_position: nalgebra::Vector2<f32>,
    current_position: nalgebra::Vector2<f32>,
    base_transform: nalgebra::Affine2<f32>,
    current_transform: nalgebra::Affine2<f32>,
}

#[derive(Debug)]
struct OngoingAnimation {
    id: u64,
    start_time: f32,
    duration: f32,
    transform: Transform,
    animate_function: AnimateFunction,
}

fn map_animate_progress(f: AnimateFunction, progress: f32) -> f32 {
    match f {
        AnimateFunction::Step => {
            if progress <= 0.5 {
                0.0
            } else {
                1.0
            }
        }
        AnimateFunction::Linear => progress,
        AnimateFunction::EaseIn => 1.0 - (progress * std::f32::consts::PI * 0.5).cos(),
        AnimateFunction::EaseOut => (progress * std::f32::consts::PI * 0.5).sin(),
        AnimateFunction::EaseInOut => (1.0 + ((progress - 0.5) * std::f32::consts::PI).sin()) * 0.5,
    }
}

impl OngoingAnimation {
    pub fn apply_transform(&self, time: f32, obj: &mut VisibleObject) -> bool {
        let elapsed = time - self.start_time;
        if elapsed < 0. {
            return false;
        }
        let (progress, done) = if elapsed >= self.duration {
            (1., true)
        } else {
            (elapsed / self.duration, false)
        };
        let p = map_animate_progress(self.animate_function, progress);

        match self.transform {
            Transform::Move((dx, dy)) => {
                let d = nalgebra::Vector2::new(dx, dy);
                if done {
                    obj.base_position += d;
                }
                obj.current_position += d * p;
            }
            Transform::Scale(s) => {
                if done {
                    obj.base_transform *= nalgebra::Affine2::from_matrix_unchecked(
                        nalgebra::Scale2::new(s, s).to_homogeneous(),
                    );
                }
                obj.current_transform *= nalgebra::Affine2::from_matrix_unchecked(
                    nalgebra::Scale2::new(s * p + 1.0 - p, s * p + 1.0 - p).to_homogeneous(),
                );
            }
            Transform::Rotate(phi) => {
                if done {
                    obj.base_transform *= nalgebra::Affine2::from_matrix_unchecked(
                        nalgebra::Rotation2::new(phi).to_homogeneous(),
                    );
                }
                obj.current_transform *= nalgebra::Affine2::from_matrix_unchecked(
                    nalgebra::Rotation2::new(phi * p).to_homogeneous(),
                );
            }
        };
        done
    }
}

struct ReplayState {
    replay: Replay,
    objects: HashMap<u64, VisibleObject>,
    object_id_by_z_index: Vec<HashSet<u64>>,
    next_event_idx: usize,
    time: f32,
    highlighted_log: std::ops::Range<usize>,
    animations: Vec<OngoingAnimation>,
}

#[derive(Default)]
struct UpdateResult {
    reset: bool,
    changed: Vec<u64>,
    created: Vec<u64>,
    deleted: Vec<u64>,
    highlighted_log: std::ops::Range<usize>,
    unhighlighted_log: std::ops::Range<usize>,
}

impl ReplayState {
    fn new(replay: Replay) -> Self {
        Self {
            replay,
            objects: HashMap::new(),
            object_id_by_z_index: vec![HashSet::new(); 256],
            next_event_idx: 0,
            time: 0.,
            animations: vec![],
            highlighted_log: 0..0,
        }
    }
    // Returns the ids of the objects that need updating.
    fn update(&mut self, new_time: f32) -> UpdateResult {
        if self.time > new_time {
            let deleted: Vec<u64> = self.objects.keys().cloned().collect();
            let unhighlight = self.highlighted_log.clone();
            self.reset();
            let mut upd = self.update(new_time);
            upd.reset = true;
            upd.deleted = deleted;
            upd.unhighlighted_log = unhighlight;
            if !upd.highlighted_log.is_empty() {
                upd.highlighted_log.start = upd.highlighted_log.end - 1;
            }
            return upd;
        }
        self.time = new_time;
        let mut res = UpdateResult::default();
        let old_hilog = self.highlighted_log.clone();
        while self.next_event_idx < self.replay.events.len()
            && self.replay.events[self.next_event_idx].start_time <= new_time
        {
            self.process_event(self.next_event_idx, &mut res);
            self.next_event_idx += 1;
        }
        if self.highlighted_log != old_hilog {
            self.highlighted_log.start = old_hilog.end;
            res.unhighlighted_log = old_hilog;
            res.highlighted_log = self.highlighted_log.clone();
        }
        res.changed = self.animations.iter().map(|a| a.id).collect();
        self.animate();
        res
    }
    fn animate(&mut self) {
        for (_, obj) in self.objects.iter_mut() {
            obj.current_position = obj.base_position;
            obj.current_transform = obj.base_transform;
        }
        self.animations.retain(|an| {
            let t = self
                .objects
                .get_mut(&an.id)
                .map(|obj| an.apply_transform(self.time, obj));
            t.map_or(false, |x| !x)
        });
    }
    fn reset(&mut self) {
        self.highlighted_log = 0..0;
        self.objects.clear();
        for o in self.object_id_by_z_index.iter_mut() {
            o.clear()
        }
        self.next_event_idx = 0;
        self.time = 0.;
        self.animations.clear();
    }
    fn process_event(&mut self, idx: usize, res: &mut UpdateResult) {
        let event = &self.replay.events[idx];
        match &event.event {
            Event::Create {
                id,
                geometry,
                position,
                z_index,
            } => {
                self.objects.insert(
                    *id,
                    VisibleObject {
                        base_position: nalgebra::Vector2::new(position.0, position.1),
                        base_transform: nalgebra::Affine2::identity(),
                        current_transform: nalgebra::Affine2::identity(),
                        current_position: nalgebra::Vector2::new(position.0, position.1),
                        geometry: geometry.clone(),
                        z_index: *z_index,
                    },
                );
                self.object_id_by_z_index[*z_index as usize].insert(*id);
                res.created.push(*id);
            }
            Event::Destroy { id } => {
                if let Some(o) = self.objects.remove(id) {
                    self.object_id_by_z_index
                        .get_mut(o.z_index as usize)
                        .map(|m| m.remove(id));
                    res.deleted.push(*id);
                }
            }
            &Event::Transform {
                id,
                transform,
                duration,
                animate_function,
            } => {
                self.animations.push(OngoingAnimation {
                    id,
                    start_time: event.start_time,
                    duration,
                    animate_function,
                    transform,
                });
            }
            Event::Log { .. } => {
                self.highlighted_log.end += 1;
            }
            Event::TickMarker => {
                log::warn!("Not implemented: TickMarker");
            }
        }
    }
    fn get_log(&self) -> impl Iterator<Item = &str> {
        self.replay.events.iter().filter_map(|e| match &e.event {
            Event::Log { line } => Some(line.as_str()),
            _ => None,
        })
    }
}

async fn fetch_text(url: &str) -> String {
    let fut = wasm_bindgen_futures::JsFuture::from;
    let resp = fut(web_sys::window().unwrap().fetch_with_str(url))
        .await
        .unwrap()
        .dyn_into::<Response>()
        .unwrap();
    let value = fut(resp.text().unwrap()).await.unwrap();
    String::try_from(value).unwrap()
}

fn parse_vis_line(l: &str) -> Option<&str> {
    let [_, _, vis, rest] = textapi::split(l);
    if vis != "vis" {
        return None;
    }
    Some(rest)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn run(replay_url: &str) {
    wasm_logger::init(wasm_logger::Config::default());
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));

    log::info!("Fetching {replay_url}");
    let j = fetch_text(replay_url).await;
    log::info!("Received replay : {}", j.len());
    let document = window().unwrap().document().unwrap();
    let mut events = Vec::<TimedEvent>::new();
    let mut time = 0f32;
    for log_line in j.lines() {
        match parse_vis_line(log_line) {
            Some(vis) => {
                let ev = match serde_hjson::from_str::<TimedEvent>(vis) {
                    Ok(ev) => ev,
                    Err(e) => {
                        log::error!("Failed to decode vis line: {e}");
                        continue;
                    }
                };
                time = ev.end_time();
                events.push(ev);
            }
            None => {
                events.push(TimedEvent {
                    start_time: time,
                    event: Event::Log {
                        line: log_line.to_owned(),
                    },
                });
            }
        }
    }
    let replay = Replay::new(events);
    let slider = get_element_by_id_unchecked::<HtmlInputElement>("progress-slider");
    let playpause = elem("playpause");
    let progress_cb = {
        let slider = slider.clone();
        Box::new(move |progress: f32| {
            slider.set_value(format!("{progress}").as_str());
        })
    };
    let handler = Rc::new(RefCell::new(MyHandler::new(
        document,
        get_element_by_id_unchecked("canvas"),
        get_element_by_id_unchecked("log"),
        get_element_by_id_unchecked("inner-svg"),
        ReplayState::new(replay),
        progress_cb,
    )));
    {
        let slider1 = slider.clone();
        let handler = handler.clone();
        let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let val = slider1.value().parse::<f32>().unwrap();
            handler
                .borrow_mut()
                .on_user_event(UserEvent::ProgressChange(val));
            //on_user_event.borrow()(UserEvent::ProgressChange(val));
        });
        slider.set_oninput(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    {
        let playpause1 = playpause.clone();
        let handler = handler.clone();
        let mut paused = true;
        let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            paused = !paused;
            if paused {
                playpause1.set_inner_text("▶️");
            } else {
                playpause1.set_inner_text("⏸️");
            }
            handler.borrow_mut().on_user_event(UserEvent::PlayPause);
        });
        playpause.set_onclick(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    //handler.borrow_mut().on_draw();
    // TODO: make this actually not use CPU.
    anim(move || handler.borrow_mut().on_draw());
}

struct Timer {
    start_time: f64,
    performance: web_sys::Performance,
}

impl Timer {
    fn new() -> Self {
        let performance = window().unwrap().performance().unwrap();
        Self {
            start_time: performance.now(),
            performance,
        }
    }
    fn secs_elapsed(&self) -> f64 {
        (self.performance.now() - self.start_time) / 1000.
    }
}

struct Surface {
    document: Document,
    size_x: f32,
    size_y: f32,
    groups_by_z: Vec<SvgElement>,
    canvas: SvgElement,
    svg_group: SvgElement,
}

impl Surface {
    pub fn new(document: Document, canvas: SvgElement, svg_group: SvgElement) -> Self {
        let mut groups_by_z = Vec::with_capacity(256);
        for _ in 0..256 {
            let group: SvgElement = document
                .create_element_ns(Some("http://www.w3.org/2000/svg"), "g")
                .unwrap()
                .dyn_into()
                .unwrap();
            svg_group.append_child(&group.clone()).unwrap();
            groups_by_z.push(group);
        }
        let w = canvas.client_width();
        let h = canvas.client_height();
        let w = w.min(h);
        Self {
            document,
            groups_by_z,
            canvas,
            svg_group,
            size_x: w as f32,
            size_y: w as f32,
        }
    }
    pub fn update_size(&mut self) {
        let w = self.canvas.client_width();
        let h = self.canvas.client_height();
        let w = w.min(h);
        self.size_x = w as f32;
        self.size_y = w as f32;
        self.svg_group
            .set_attribute("transform", &format!("scale({} {})", w, w))
            .unwrap();
    }
}
struct MyHandler {
    replay_state: ReplayState,
    timer: Timer,
    prev_time: f32,
    progress_time: f32,
    progress_callback: Box<dyn FnMut(f32)>,
    paused: bool,
    surface: Surface,
    object_elements: HashMap<u64, SvgElement>,
    log_list: HtmlElement,
}

impl MyHandler {
    pub fn new(
        document: Document,
        canvas: SvgElement,
        log_list: HtmlElement,
        svg_group: SvgElement,
        replay_state: ReplayState,
        progress_callback: Box<dyn FnMut(f32)>,
    ) -> Self {
        for line in replay_state.get_log() {
            log_list
                .append_child(&make_log_element(&document, line))
                .unwrap();
        }
        Self {
            replay_state,
            timer: Timer::new(),
            prev_time: 0.0,
            progress_time: 0.0,
            paused: true,
            progress_callback,
            surface: Surface::new(document.clone(), canvas, svg_group),
            object_elements: HashMap::default(),
            log_list,
        }
    }
    fn time_delta(&mut self) -> f32 {
        let t = self.timer.secs_elapsed() as f32;
        let d = t - self.prev_time;
        self.prev_time = t;
        d
    }
    fn on_draw(&mut self) {
        self.surface.update_size();
        if !self.paused {
            self.progress_time += self.time_delta();
            (self.progress_callback)(self.progress_time / self.replay_state.replay.duration);
        } else {
            self.time_delta();
        }
        let update_result = self.replay_state.update(self.progress_time);
        for id in update_result.deleted {
            let Some(elt) = self.object_elements.get(&id) else {
                continue;
            };
            elt.remove();
            self.object_elements.remove(&id).inspect(|e| e.remove());
        }
        for id in update_result.created {
            self.object_elements.insert(
                id,
                create_element(&self.surface, self.replay_state.objects.get(&id).unwrap()),
            );
        }
        for id in update_result.changed {
            let (Some(elt), Some(obj)) = (
                self.object_elements.get_mut(&id),
                self.replay_state.objects.get(&id),
            ) else {
                continue;
            };
            elt.set_attribute("transform", &make_transform_attribute(obj))
                .unwrap();
        }
        for i in update_result.unhighlighted_log {
            self.unhighligh_log_element(i);
        }
        for i in update_result.highlighted_log {
            self.highlight_log_element(i);
        }
    }
    fn unhighligh_log_element(&self, index: usize) {
        let Some(child) = self.log_list.children().item(index as u32) else {
            return;
        };
        child.class_list().remove_1("highlight").unwrap();
    }
    fn highlight_log_element(&self, index: usize) {
        let Some(child) = self.log_list.children().item(index as u32) else {
            return;
        };
        let mut opts = ScrollIntoViewOptions::new();
        opts.behavior(ScrollBehavior::Auto)
            .block(ScrollLogicalPosition::Center)
            .inline(ScrollLogicalPosition::Center);
        child.scroll_into_view_with_scroll_into_view_options(&opts);
        child.class_list().add_1("highlight").unwrap();
    }
    fn on_user_event(&mut self, user_event: UserEvent) {
        match user_event {
            UserEvent::ProgressChange(progress) => {
                self.progress_time = self.replay_state.replay.duration * progress;
            }
            UserEvent::PlayPause => {
                if self.progress_time >= self.replay_state.replay.duration {
                    self.progress_time = 0.0;
                }
                self.paused = !self.paused;
            }
        }
    }
}

#[derive(Debug)]
enum UserEvent {
    ProgressChange(f32),
    PlayPause,
}

fn get_element_by_id_unchecked<T: JsCast>(id: &str) -> T {
    web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .get_element_by_id(id)
        .unwrap()
        .dyn_into()
        .unwrap()
}

fn elem(id: &str) -> HtmlElement {
    get_element_by_id_unchecked(id)
}

fn anim<F: Fn() + Clone + 'static>(cb: F) {
    cb();
    let closure =
        wasm_bindgen::closure::Closure::<dyn FnMut()>::new(Box::new(move || anim(cb.clone())));
    window()
        .unwrap()
        .request_animation_frame(closure.as_ref().unchecked_ref())
        .unwrap();
    closure.forget();
}

fn create_element(surface: &Surface, obj: &VisibleObject) -> SvgElement {
    let document = &surface.document;
    let group = svg::group(document, (obj.base_position.x, obj.base_position.y));
    for g in obj.geometry.iter() {
        let elt: SvgElement = match g {
            geom::Geom::Circle {
                center,
                radius,
                fill_color,
                stroke_color,
                thickness,
            } => {
                log::info!("Create circle");
                svg::circle(
                    document,
                    *center,
                    *radius,
                    *fill_color,
                    *stroke_color,
                    *thickness,
                )
                .into()
            }
            geom::Geom::Line {
                from,
                to,
                thickness,
                color,
            } => svg::line(document, *from, *to, *thickness, *color).into(),
            geom::Geom::Text {
                text,
                position,
                size,
                color,
            } => svg::text(document, position.0, position.1, *size, text, *color),
            geom::Geom::Polygon {
                vs,
                fill_color,
                stroke_color,
                thickness,
            } => svg::polygon(document, vs, *fill_color, *stroke_color, *thickness).into(),
        };
        group.append_child(&elt).unwrap();
    }
    let z = (obj.z_index as usize).min(surface.groups_by_z.len());
    surface.groups_by_z[z].append_child(&group).unwrap();
    group
}

fn make_transform_attribute(obj: &VisibleObject) -> String {
    let matrix = obj.current_transform.matrix();
    let scale = matrix.determinant().sqrt();
    let angle = (matrix.get((0, 0)).unwrap() / scale)
        .clamp(-1., 1.)
        .acos()
        * 180.
        / std::f32::consts::PI;
    format!(
        "translate({} {}) scale({}) rotate({})",
        obj.current_position.x, obj.current_position.y, scale, angle
    )
}

fn make_log_element(document: &Document, line: &str) -> HtmlElement {
    let li = document.create_element("li").unwrap();
    li.set_text_content(Some(line));
    li.dyn_into().unwrap()
}
