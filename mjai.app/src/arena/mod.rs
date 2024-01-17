mod board;
mod game;
mod result;
mod single_match;

pub use board::Board;
pub use result::{GameResult, KyokuEndState};

use crate::py_helper::add_submodule;
use single_match::Match;

use pyo3::prelude::{PyModule, PyResult, Python};

pub(crate) fn register_module(py: Python<'_>, prefix: &str, super_mod: &PyModule) -> PyResult<()> {
    let m = PyModule::new(py, "arena")?;
    m.add_class::<Match>()?;
    add_submodule(py, prefix, super_mod, m)
}
