pub fn write_ucfb_chunk(buf: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    buf.extend_from_slice(tag);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

pub fn build_synthetic_lvl(scripts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut inner = Vec::new();
    for &(name, body) in scripts {
        let mut scr_body = Vec::new();
        let mut name_nul = name.as_bytes().to_vec();
        name_nul.push(0);
        write_ucfb_chunk(&mut scr_body, b"NAME", &name_nul);
        write_ucfb_chunk(&mut scr_body, b"INFO", &[1u8]);
        let mut body_nul = body.to_vec();
        body_nul.push(0);
        write_ucfb_chunk(&mut scr_body, b"BODY", &body_nul);
        write_ucfb_chunk(&mut inner, b"scr_", &scr_body);
    }
    let mut lvl_payload = vec![0u8; 8];
    lvl_payload.extend_from_slice(&inner);
    let mut root_body = Vec::new();
    write_ucfb_chunk(&mut root_body, b"lvl_", &lvl_payload);
    let mut file = Vec::new();
    write_ucfb_chunk(&mut file, b"ucfb", &root_body);
    file
}
