use pyo3::pyclass;

#[pyclass]
#[derive(Debug, Clone)]
pub struct HmacKeyStore {
    access_key: String,
    secret_key: String,

}

impl HmacKeyStore {
    pub fn new(access_key: String, secret_key: String) -> Self {
        HmacKeyStore {
            access_key,
            secret_key,
        }
    }

    pub fn get_access_key(&self) -> &str {
        &self.access_key
    }

    pub fn get_secret_key(&self) -> &str {
        &self.secret_key
    }
}
impl Default for HmacKeyStore {
    fn default() -> Self {
        HmacKeyStore {
            access_key: String::new(),
            secret_key: String::new(),
        }
    }
}