//! Component-only native exercise runner core (rust-22): the sandboxed
//! child lifecycle, CPU, memory and wall limits, repeated CPU-time scaling
//! measurements, the complexity decision, the comparison modes and grading.
//! No production route selects it yet.

pub mod compare;
pub mod decide;
pub mod plant;
pub mod py;
pub mod runner;
pub mod synthetic;
