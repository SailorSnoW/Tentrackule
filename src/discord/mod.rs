mod bot;
pub mod commands;
pub mod ddragon;
pub mod grade;
pub mod image_gen;
pub mod scorecard;

pub use bot::{Data, create_framework};
pub use image_gen::ImageGenerator;
