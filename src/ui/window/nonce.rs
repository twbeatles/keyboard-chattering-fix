use std::sync::atomic::{AtomicU64, Ordering};

/// Secret posted with `WM_UPDATE_EXIT`: a constant message id can be sent by
/// any process in the session, but only the holder of this process's nonce
/// (our own update/elevation worker) may exit it for replacement (SEC-001).
pub(super) static UPDATE_EXIT_NONCE: AtomicU64 = AtomicU64::new(0);

/// Nonce for this process's `WM_UPDATE_EXIT`, for the two worker threads
/// that post it (`update`, `elevation`). Zero until `on_create` mints one.
pub(crate) fn update_exit_nonce() -> u64 {
    UPDATE_EXIT_NONCE.load(Ordering::Acquire)
}

/// Whether `wparam` carries this process's exit nonce (SEC-001).
pub(super) fn update_exit_authenticated(wparam: usize) -> bool {
    let nonce = UPDATE_EXIT_NONCE.load(Ordering::Acquire);
    nonce != 0 && wparam as u64 == nonce
}

/// 64-bit secret from the OS RNG (`None` when unavailable).
fn system_nonce() -> Option<u64> {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    let mut bytes = [0u8; 8];
    unsafe {
        if BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG).is_err() {
            return None;
        }
    }
    Some(u64::from_ne_bytes(bytes))
}

/// Mint the exit nonce once, at window creation (UI thread, before any
/// worker can post). Falls back to hashed process-unique values when the
/// OS RNG is unavailable; never zero.
pub(super) fn mint_update_exit_nonce() {
    let nonce = system_nonce().unwrap_or_else(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        hasher.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
        );
        hasher.finish()
    });
    UPDATE_EXIT_NONCE.store(if nonce == 0 { 1 } else { nonce }, Ordering::Release);
}
