use std::collections::HashMap;

use pyo3::{exceptions, prelude::*, types::{PyDict, PyList}};


pub fn parse_hmac_list(
    py: Python,
    hmac_list: &PyObject,
) -> PyResult<HashMap<String, String>> {
    // let list: Vec<HashMap<String, String>> = hmac_list.try_into().unwrap();
    let list: Vec<HashMap<String, String>> = hmac_list.extract(py).expect("dict mismatch");
    let mut map = HashMap::new();

    for item in list {
        let access_key: String = item.get("access_key")
            .ok_or_else(|| exceptions::PyKeyError::new_err("access_key not found"))?
            .to_string();
        let secret_key: String = item.get("secret_key")
            .ok_or_else(|| exceptions::PyKeyError::new_err("secret_key not found"))?
            .to_string();
        map.insert(access_key, secret_key);
    }
    Ok(map)

}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use pyo3::types::{IntoPyDict, PyList};
//     use pyo3::Python;

//     #[test]
//     fn test_parse_hmac_list_success() {
//         Python::with_gil(|py| {
//             let dict1 = [("access_key", "key1"), ("secret_key", "secret1")].into_py_dict(py);
//             let dict2 = [("access_key", "key2"), ("secret_key", "secret2")].into_py_dict(py);
//             let dict1 = dict1.expect("Failed to create dict1");
//             let dict2 = dict2.expect("Failed to create dict2");
//             let py_list: PyObject = PyList::new(py, &[dict1, dict2]).unwrap().into();

//             let result = parse_hmac_list(py, &py_list).expect("Parsing should succeed");
//             assert_eq!(result.get("key1").unwrap(), "secret1");
//             assert_eq!(result.get("key2").unwrap(), "secret2");
//         });
//     }

//     // #[test]
//     // fn test_parse_hmac_list_missing_key() {
//     //     Python::with_gil(|py| {
//     //         let dict1 = [("access_key", "key1")].into_py_dict(py); // missing secret_key
//     //         let dict1 = dict1.unwrap();
//     //         let py_list = PyList::new(py, &[&dict1]);

//     //         let result = parse_hmac_list(py, &py_list);
//     //         assert!(result.is_err(), "Should error if secret_key is missing");
//     //     });
//     // }
// }