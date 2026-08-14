use bloomery_core::vram::free_vram_bytes;

#[test]
fn parses_nvidia_smi_mib() {
    let out = free_vram_bytes(|_, _| Ok("14558\n".to_string()));
    assert_eq!(out, Some(14558 * 1024 * 1024));
}

#[test]
fn multi_gpu_takes_first_line() {
    let out = free_vram_bytes(|_, _| Ok("14558\n8192\n".to_string()));
    assert_eq!(out, Some(14558 * 1024 * 1024));
}

#[test]
fn missing_binary_is_none_not_zero() {
    let out = free_vram_bytes(|_, _| Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
    assert_eq!(out, None);
}

#[test]
fn garbage_output_is_none() {
    let out = free_vram_bytes(|_, _| Ok("N/A\n".to_string()));
    assert_eq!(out, None);
}

#[test]
fn overflow_mib_is_none() {
    let out = free_vram_bytes(|_, _| Ok("18446744073709551615\n".to_string()));
    assert_eq!(out, None);
}
