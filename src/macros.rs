#[macro_export]
macro_rules! async_write {
    (flush => $dst:expr) => {
        $dst
            .flush()
            .await
            .unwrap();
    };
    (as [u8] => $dst:expr, $bytes:expr) => {
        $dst
            .write_all($bytes)
            .await
            .unwrap();
    };
    ($dst:expr, $($arg:tt)*) => {
        $crate::async_write!(as [u8] => $dst, format!($($arg)*).as_bytes());
    };
}
