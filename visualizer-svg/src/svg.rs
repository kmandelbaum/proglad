use proglad_api::visualize::Color;
use wasm_bindgen::JsCast;
use web_sys::{Document, SvgCircleElement, SvgElement, SvgLineElement, SvgTextElement, SvgPolygonElement};

pub fn circle(
    document: &Document,
    (cx, cy): (f32, f32),
    r: f32,
    fill_color: Color,
    stroke_color: Color,
    thickness: f32,
) -> SvgCircleElement {
    let elt: SvgCircleElement = document
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "circle")
        .unwrap()
        .dyn_into()
        .unwrap();
    let _ = elt.set_attribute("r", &format!("{r}"));
    let _ = elt.set_attribute("cx", &format!("{cx}"));
    let _ = elt.set_attribute("cy", &format!("{cy}"));
    let _ = elt.set_attribute("fill", &html_color(fill_color));
    let _ = elt.set_attribute("stroke", &html_color(stroke_color));
    let _ = elt.set_attribute("stroke-width", &format!("{thickness}"));
    elt
}
pub fn polygon(document: &Document, vs: &[(f32, f32)], fill_color: Color, stroke_color: Color, thickness: f32)
-> SvgPolygonElement {
    let elt: SvgPolygonElement = document
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "polygon")
        .unwrap()
        .dyn_into()
        .unwrap();
    let mut points_str = String::new();
    for (x, y) in vs {
        use std::fmt::Write;
        write!(&mut points_str, "{x},{y} ").unwrap();
    }
    let _ = elt.set_attribute("points", &points_str);
    let _ = elt.set_attribute("fill", &html_color(fill_color));
    let _ = elt.set_attribute("stroke", &html_color(stroke_color));
    let _ = elt.set_attribute("stroke-width", &format!("{thickness}"));
    elt
}
pub fn text(
    document: &Document,
    x: f32,
    y: f32,
    size: f32,
    text: &str,
    color: Color,
) -> SvgElement {
    // A hack to add a group element as font sized don't work correctly.
    let group: SvgElement = document
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "g")
        .unwrap()
        .dyn_into()
        .unwrap();

    let _ = group.set_attribute(
        "transform",
        &format!("translate({x} {y}) scale({} {})", size / 12.0, size / 12.0),
    );
    let elt: SvgTextElement = document
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "text")
        .unwrap()
        .dyn_into()
        .unwrap();
    elt.set_attribute("font-size", "12").unwrap();
    elt.set_attribute("stroke", &html_color(color)).unwrap();
    elt.set_text_content(Some(text));
    group.append_child(&elt).unwrap();
    group
}

pub fn line(
    document: &Document,
    from: (f32, f32),
    to: (f32, f32),
    thickness: f32,
    color: Color,
) -> SvgLineElement {
    let line: SvgLineElement = document
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "line")
        .unwrap()
        .dyn_into()
        .unwrap();
    line.set_attribute("x1", &format!("{}", from.0)).unwrap();
    line.set_attribute("y1", &format!("{}", from.1)).unwrap();
    line.set_attribute("x2", &format!("{}", to.0)).unwrap();
    line.set_attribute("y2", &format!("{}", to.1)).unwrap();
    line.set_attribute("stroke", &html_color(color)).unwrap();
    line.set_attribute("stroke-width", &format!("{thickness}"))
        .unwrap();
    line
}

pub fn group(document: &Document, translate: (f32, f32)) -> SvgElement {
    let group: SvgElement = document
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "g")
        .unwrap()
        .dyn_into()
        .unwrap();
    let _ = group.set_attribute(
        "transform",
        &format!("translate({} {})", translate.0, translate.1),
    );
    group
}

pub fn html_color(c: Color) -> String {
    format!("rgba({},{},{},{})", c.r * 255., c.g * 255., c.b * 255., c.a)
}
