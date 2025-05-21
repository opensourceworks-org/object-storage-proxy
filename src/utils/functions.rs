use pyo3::prelude::*;
use pyo3::types::{IntoPyDict, PyAny, PyFunction};
use tracing::debug;

pub(crate) fn callable_accepts_request(py: Python<'_>, callable: &PyObject) -> PyResult<bool> {

    let inspect = py.import("inspect")?;
    let signature = inspect.call_method1("signature", (callable.to_owned(),))?;
    let parameters = signature.getattr("parameters")?;
    dbg!(&parameters);
    let parameters = parameters.call_method0("items")?;

    
    for p in parameters.try_iter()? {
        let (name, param) = p?.extract::<(String, PyObject)>()?;
        let annotation = param.getattr(py, "annotation")?;
        debug!("Param: {}", name);
        let arg_type = annotation.to_string();
        debug!("Annotation: {}", &arg_type);
        if name == "request" && arg_type.contains("dict"){
            return Ok(true)
        }

    }

    Ok(false)
}