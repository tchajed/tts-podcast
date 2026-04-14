/// Short random IDs (12 chars alphanumeric) for database records.
pub fn new_id() -> String {
    nanoid::nanoid!(12, &nanoid::alphabet::SAFE)
}

/// Longer token (21 chars) for feed authentication tokens.
pub fn new_token() -> String {
    nanoid::nanoid!(21, &nanoid::alphabet::SAFE)
}
