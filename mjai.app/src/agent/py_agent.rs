use super::{BatchAgent, MjaiLogBatchAgent};
use std::str::FromStr;

use anyhow::{bail, Error, Result};
use pyo3::prelude::*;

enum EngineType {
    MjaiLog,
}

impl FromStr for EngineType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mjai-log" => Ok(Self::MjaiLog),
            v => bail!("unknown engine type {v}"),
        }
    }
}

pub fn new_py_agent(engine: PyObject, player_ids: &[u8]) -> Result<Box<dyn BatchAgent>> {
    let engine_type = Python::with_gil(|py| {
        engine
            .as_ref(py)
            .getattr("engine_type")?
            .extract::<&str>()?
            .parse()
    })?;
    let agent = match engine_type {
        EngineType::MjaiLog => Box::new(MjaiLogBatchAgent::new(engine, player_ids)?) as _,
    };
    Ok(agent)
}
