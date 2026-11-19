//! Tidefire's engine: the GPU fire/smoke and water solvers, the renderer, and
//! the scene model with its presets. Used natively by the command line tool and
//! compiled to WebAssembly (WebGPU) for the interface in the desktop app.

pub mod engine;
pub mod gpu;
pub mod params;
pub mod render;
pub mod smoke;
pub mod sparks;
pub mod water;

pub use wgpu;
