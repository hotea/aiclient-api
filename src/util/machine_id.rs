use sha2::{Sha256, Digest};

/// Generate a machine ID from the primary MAC address, SHA256 hashed.
/// Falls back to a random UUID persisted in config dir if no MAC found.
pub fn get_machine_id() -> String {
    if let Ok(Some(addr)) = mac_address::get_mac_address() {
        let mut hasher = Sha256::new();
        hasher.update(addr.to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    } else {
        let path = super::xdg::config_dir().join("machine_id");
        if let Ok(id) = std::fs::read_to_string(&path) {
            return id.trim().to_string();
        }
        let id = uuid::Uuid::new_v4().to_string();
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, &id);
        id
    }
}
