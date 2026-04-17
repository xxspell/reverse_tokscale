#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalEvent {
    pub timestamp: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayPlan {
    pub events: Vec<InternalEvent>,
}
