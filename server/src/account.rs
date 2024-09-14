pub fn validate_account_name(name: &str) -> Result<(), String> {
    if !(2..=20).contains(&name.len()) {
        return Err(format!(
            "Failed account name length check: 0 <= length={} <= 20",
            name.len()
        ));
    }
    for c in name.chars() {
        if !char_allowed(c) {
            return Err(format!(
                "Disallowed characters found in account name: '{c}' code={:x}",
                c as u32
            ));
        }
    }
    Ok(())
}

fn char_allowed(c: char) -> bool {
    c.is_alphanumeric() && c.is_ascii() || c == '-' || c == '_'
}
