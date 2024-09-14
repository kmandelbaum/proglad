use serde::de::Error;
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Serialize for Color {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let byte = |c: f32| (c.clamp(0., 1.) * 255.).round() as u8;
        serializer.serialize_str(&format!(
            "{:02x}{:02x}{:02x}{:02x}",
            byte(self.r),
            byte(self.g),
            byte(self.b),
            byte(self.a)
        ))
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.len() != 8 {
            return Err(D::Error::custom("color string must have length 8"));
        }
        let mut nums = [0u8; 4];
        for i in 0..4 {
            nums[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(D::Error::custom)?;
        }
        let f = |n: u8| -> f32 { n as f32 / 255. };
        Ok(Color {
            r: f(nums[0]),
            g: f(nums[1]),
            b: f(nums[2]),
            a: f(nums[3]),
        })
    }
}

pub mod geom {
    use super::{Color, Deserialize, Serialize};

    fn default_radius() -> f32 {
        0.1
    }

    fn default_text_height() -> f32 {
        0.05
    }

    fn default_stroke_color() -> Color {
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }

    fn default_thickness() -> f32 {
        0.005
    }

    fn default_fill_color() -> Color {
        Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.0,
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub enum Geom {
        #[serde(rename = "line")]
        Line {
            #[serde(rename = "p1")]
            from: (f32, f32),
            #[serde(rename = "p2")]
            to: (f32, f32),
            #[serde(default = "default_thickness")]
            #[serde(rename = "t")]
            thickness: f32,
            #[serde(default = "default_stroke_color")]
            #[serde(rename = "s")]
            color: Color,
        },
        #[serde(rename = "circle")]
        Circle {
            #[serde(default = "super::zero")]
            #[serde(rename = "p")]
            center: (f32, f32),
            #[serde(default = "default_radius")]
            #[serde(rename = "r")]
            radius: f32,
            #[serde(default = "default_fill_color")]
            #[serde(rename = "f")]
            fill_color: Color,
            #[serde(default = "default_stroke_color")]
            #[serde(rename = "s")]
            stroke_color: Color,
            #[serde(default = "default_thickness")]
            #[serde(rename = "t")]
            thickness: f32,
        },
        #[serde(rename = "poly")]
        Polygon {
            vs: Vec<(f32, f32)>,
            #[serde(default = "default_fill_color")]
            #[serde(rename = "f")]
            fill_color: Color,
            #[serde(default = "default_stroke_color")]
            #[serde(rename = "s")]
            stroke_color: Color,
            #[serde(default = "default_thickness")]
            #[serde(rename = "t")]
            thickness: f32,
        },
        #[serde(rename = "text")]
        Text {
            #[serde(rename = "v")]
            text: String,
            #[serde(rename = "p")]
            position: (f32, f32),
            #[serde(default = "default_text_height")]
            #[serde(rename = "t")]
            size: f32,
            #[serde(default = "default_stroke_color")]
            #[serde(rename = "s")]
            color: Color,
        },
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Transform {
    #[serde(rename = "mv")]
    Move((f32, f32)),
    #[serde(rename = "rot")]
    Rotate(f32),
    #[serde(rename = "scale")]
    Scale(f32),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Event {
    #[serde(rename = "create")]
    Create {
        id: u64,
        #[serde(default = "zero")]
        #[serde(rename = "p")]
        position: (f32, f32),
        #[serde(default = "default_z")]
        #[serde(rename = "z")]
        z_index: u8,
        #[serde(rename = "geom")]
        geometry: Vec<geom::Geom>,
    },
    #[serde(rename = "destroy")]
    Destroy {
        id: u64,
    },
    #[serde(rename = "transform")]
    Transform {
        id: u64,
        #[serde(rename = "d")]
        duration: f32,
        #[serde(default)]
        #[serde(rename = "f")]
        animate_function: AnimateFunction,
        #[serde(flatten)]
        transform: Transform,
    },
    #[serde(rename = "log")]
    Log {
        line: String,
    },
    // Can be used to denote moves.
    TickMarker,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum AnimateFunction {
    #[serde(rename = "step")]
    Step,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "easin")]
    EaseIn,
    #[serde(rename = "easeout")]
    EaseOut,
    #[serde(rename = "easeinout")]
    #[default]
    EaseInOut,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedEvent {
    #[serde(rename = "t")]
    pub start_time: f32,
    #[serde(flatten)]
    pub event: Event,
}

impl Event {
    pub fn duration(&self) -> f32 {
        match self {
            Event::Transform { duration, .. } => *duration,
            _ => 0.0,
        }
    }
}

impl TimedEvent {
    pub fn end_time(&self) -> f32 {
        self.start_time + self.event.duration()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    pub events: Vec<TimedEvent>,
    pub duration: f32,
}

impl Replay {
    pub fn new(events: Vec<TimedEvent>) -> Self {
        Self {
            duration: Self::duration(&events),
            events,
        }
    }
    fn duration(events: &[TimedEvent]) -> f32 {
        events
            .iter()
            .map(|e| e.end_time())
            .max_by(|x, y| x.partial_cmp(y).unwrap())
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Clone)]
pub enum DecodeError {
    UnknownCommand(String),
    UnrecognizedGeometryType(String),
    UnrecognizedAnimateFunction(String),
    UnrecognizedTransform(String),
    ParseError(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DecodeError {}

fn zero() -> (f32, f32) {
    (0., 0.)
}

fn default_z() -> u8 {
    1
}
