use pyo3::prelude::*;
use tracing::debug;

/// Determine at runtime whether a Python callable accepts a `request` dict argument.
///
/// Uses `inspect.signature` to walk the parameter list.  Returns `true` if any
/// parameter is named `request` **and** its annotation contains `"dict"`.
///
/// This lets the proxy call the validator with or without the request dict
/// depending on what the supplied callback actually expects.
pub(crate) fn callable_accepts_request(py: Python<'_>, callable: &PyObject) -> PyResult<bool> {
    let inspect = py.import("inspect")?;
    let signature = inspect.call_method1("signature", (callable.to_owned(),))?;
    let parameters = signature.getattr("parameters")?;
    debug!(parameters = ?parameters, "inspecting callable signature");
    let parameters = parameters.call_method0("items")?;

    for p in parameters.try_iter()? {
        let (name, param) = p?.extract::<(String, PyObject)>()?;
        let annotation = param.getattr(py, "annotation")?;
        debug!("Param: {}", name);
        let arg_type = annotation.to_string();
        debug!("Annotation: {}", &arg_type);
        if name == "request" && arg_type.contains("dict") {
            return Ok(true);
        }
    }

    Ok(false)
}
