pub fn validate_bot_name(name: &str) -> Result<(), String> {
    const MAX: usize = 30;
    if !(1..=MAX).contains(&name.len()) {
        return Err(format!(
            "Failed bot name length check: 0 <= length={} <= {MAX}",
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