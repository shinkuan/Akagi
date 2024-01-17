use crate::py_helper::add_submodule;

use pyo3::prelude::*;

use riichi_tools_rs::riichi::hand::Hand;

#[pyfunction]
pub fn find_improving_tiles(tehai_tenhou: &str) -> Vec<(u8, Vec<u8>)> {
    let mut vec = vec![];

    match Hand::from_text(tehai_tenhou, true) {
        Ok(mut hand) => {
            if hand.count_tiles() == 14 {
                for dt in hand.find_shanten_improving_tiles(None) {
                    let next_hand: Vec<u8> = dt.1.iter().map(|x| x.0.get_id() - 1).collect();
                    vec.push((dt.0.unwrap().get_id() - 1, next_hand));
                }
            } else if hand.count_tiles() == 13 {
                for dt in hand.find_shanten_improving_tiles(None) {
                    let next_hand: Vec<u8> = dt.1.iter().map(|x| x.0.get_id() - 1).collect();
                    vec.push((34, next_hand));
                }
            }
        }
        _ => {}
    }

    vec
}

#[pyfunction]
pub fn calc_shanten(tehai_tenhou: &str) -> i8 {
    match Hand::from_text(tehai_tenhou, true) {
        Ok(mut hand) => {
            if hand.count_tiles() == 14 || hand.count_tiles() == 13 {
                return hand.shanten();
            } else {
                return -2;
            }
        }
        _ => return -2,
    }
}

pub(crate) fn register_module(py: Python<'_>, prefix: &str, super_mod: &PyModule) -> PyResult<()> {
    let m = PyModule::new(py, "tools")?;
    m.add_function(wrap_pyfunction!(find_improving_tiles, m)?)?;
    m.add_function(wrap_pyfunction!(calc_shanten, m)?)?;
    add_submodule(py, prefix, super_mod, m)
}
