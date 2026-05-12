use pyo3::pyclass;

#[pyclass]
#[derive(Debug, Clone, Default)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_keys() {
        let ks = HmacKeyStore::new("AK123".into(), "SK456".into());
        assert_eq!(ks.get_access_key(), "AK123");
        assert_eq!(ks.get_secret_key(), "SK456");
    }

    #[test]
    fn default_is_empty_strings() {
        let ks = HmacKeyStore::default();
        assert_eq!(ks.get_access_key(), "");
        assert_eq!(ks.get_secret_key(), "");
    }

    #[test]
    fn clone_is_independent() {
        let original = HmacKeyStore::new("AK".into(), "SK".into());
        let cloned = original.clone();
        assert_eq!(cloned.get_access_key(), original.get_access_key());
        assert_eq!(cloned.get_secret_key(), original.get_secret_key());
    }
}
