use proglad_api::visualize::*;

fn color(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color { r, g, b, a }
}

fn main() {
    let replay = Replay::new(vec![
        TimedEvent {
            start_time: 0.0,
            event: Event::Create {
                id: 1,
                geometry: vec![
                    geom::Geom::Circle {
                        center: (0.0, 0.0),
                        radius: 0.1,
                        thickness: 0.01,
                        stroke_color: color(0.1, 0.1, 0.1, 1.0),
                        fill_color: color(0.1, 0.5, 0.1, 1.0),
                    },
                    geom::Geom::Circle {
                        center: (0.03, 0.06),
                        radius: 0.05,
                        thickness: 0.01,
                        stroke_color: color(0.1, 0.1, 0.1, 1.0),
                        fill_color: color(0.5, 0.1, 0.1, 1.0),
                    },
                    geom::Geom::Line {
                        from: (0., 0.),
                        to: (0.03, 0.06),
                        thickness: 0.01,
                        color: color(0.1, 0.1, 0.5, 1.0),
                    },
                ],
                position: (0.15, 0.3),
                z_index: 10,
            },
        },
        TimedEvent {
            start_time: 0.1,
            event: Event::Transform {
                id: 1,
                duration: 2.,
                animate_function: AnimateFunction::Linear,
                transform: Transform::Rotate(std::f32::consts::PI * 2. / 3.),
            },
        },
        TimedEvent {
            start_time: 0.5,
            event: Event::Transform {
                id: 1,
                duration: 2.0,
                animate_function: AnimateFunction::EaseInOut,
                transform: Transform::Move((0.3, 0.1)),
            },
        },
    ]);
    for ev in replay.events.iter() {
        println!("{}", serde_json::to_string(ev).unwrap());
    }
    println!("{}", serde_json::to_string_pretty(&replay).unwrap());
}
