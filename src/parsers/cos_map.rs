use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::{PyResult, Python};

#[derive(FromPyObject, Debug, Clone)]
pub struct CosMapItem {
    pub host: String,
    pub port: u16,
    pub api_key: Option<String>,
}

pub(crate) fn parse_cos_map(py: Python, cos_dict: &PyObject) -> PyResult<HashMap<String, CosMapItem>> {
    let mut cos_map: HashMap<String, CosMapItem> = HashMap::new();
    
    let tuples: Vec<(String, String, u16, Option<String>)> = cos_dict.extract(py)?;
    for (bucket, host, port, api_key) in tuples {
        let host = host.to_string();
        let port = port;
        let bucket = bucket.to_string();
        let api_key = api_key.map(|s| s.to_string());

        cos_map.insert(
            bucket.clone(),
            CosMapItem {
                host: host.clone(),
                port,
                api_key: api_key.clone(),
            },
        );
    };

    Ok(cos_map)


}