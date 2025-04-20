use std::collections::HashMap;

use pyo3::exceptions::PyKeyError;
use pyo3::prelude::*;
use pyo3::{PyResult, Python};

#[pyclass]
#[derive(Debug, Clone)]
pub struct CosMapItem {
    pub host: String,
    pub port: u16,
    pub api_key: Option<String>,
    pub ttl: Option<u64>,
}

pub(crate) fn parse_cos_map(
    py: Python,
    cos_dict: &PyObject,
) -> PyResult<HashMap<String, CosMapItem>> {
    let raw_map: HashMap<String, HashMap<String, PyObject>> = cos_dict.extract(py)?;
    let mut map = HashMap::new();

    for (bucket, inner_map) in raw_map {
        let host_obj = inner_map
            .get("host")
            .ok_or_else(|| PyKeyError::new_err("Missing 'host' in COS map entry"))?;
        let host: String = host_obj.extract(py)?;

        let port_obj = inner_map
            .get("port")
            .ok_or_else(|| PyKeyError::new_err("Missing 'port' in COS map entry"))?;
        let port: u16 = port_obj.extract(py)?;

        // Optional: api_key (allow 'api_key' or 'apikey')
        let api_key =
            if let Some(val) = inner_map.get("api_key").or_else(|| inner_map.get("apikey")) {
                Some(val.extract(py)?)
            } else {
                None
            };
        let ttl = if let Some(val) = inner_map
            .get("ttl")
            .or_else(|| inner_map.get("time-to-live"))
        {
            Some(val.extract(py)?)
        } else {
            None
        };

        map.insert(
            bucket.clone(),
            CosMapItem {
                host,
                port,
                api_key,
                ttl,
            },
        );
    }

    Ok(map)
}

// #[derive(FromPyObject, Debug, Clone)]
// pub struct CosMapItem {
//     pub host: String,
//     pub port: u16,
//     pub api_key: Option<String>,
// }

// pub(crate) fn parse_cos_map(py: Python, cos_dict: &PyObject) -> PyResult<HashMap<String, CosMapItem>> {

//     let raw_map: HashMap<String, HashMap<String, PyObject>> = cos_dict.extract(py)?;
//     let mut map = HashMap::new();

//     for (bucket, inner_map) in raw_map {
//         let host_obj = inner_map.get("host")
//             .ok_or_else(|| PyKeyError::new_err("Missing 'host' in COS map entry"))?;
//         let host: String = host_obj.extract(py)?;

//         let port_obj = inner_map.get("port")
//             .ok_or_else(|| PyKeyError::new_err("Missing 'port' in COS map entry"))?;
//         let port: u16 = port_obj.extract(py)?;

//         // Optional: api_key (allow 'api_key' or 'apikey')
//         let api_key = if let Some(val) = inner_map.get("api_key").or_else(|| inner_map.get("apikey")) {
//             Some(val.extract(py)?)
//         } else {
//             None
//         };

//         map.insert(bucket.clone(), CosMapItem { host, port, api_key });
//     }

//     Ok(map)

// }
