pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, digit) in digits.char_indices() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }

        formatted.push(digit);
    }

    formatted
}
