use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, Serializer};

/// Scale factor for fixed-point arithmetic (4 decimal places)
pub const SCALE: i64 = 10_000;

/// Convert Decimal to fixed-point i64
pub fn to_fixed(d: Decimal) -> i64 {
    use rust_decimal::prelude::ToPrimitive;
    (d * Decimal::from(SCALE)).trunc().to_i64().unwrap_or(0)
}

/// Format fixed-point i64 as decimal string
fn format_fixed(value: i64) -> String {
    let is_negative = value < 0;
    // Use wrapping_abs to avoid panic on i64::MIN
    let abs_value = value.wrapping_abs() as u64;
    let whole = abs_value / SCALE as u64;
    let frac = abs_value % SCALE as u64;
    if is_negative {
        format!("-{}.{:04}", whole, frac)
    } else {
        format!("{}.{:04}", whole, frac)
    }
}

fn serialize_fixed<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format_fixed(*value))
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    #[serde(rename = "type")]
    pub tx_type: TransactionType,
    pub client: u16,
    pub tx: u32,
    pub amount: Option<Decimal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisputeState {
    #[default]
    None,
    Disputed,
    ChargedBack,
}

#[derive(Debug, Clone)]
pub struct StoredTransaction {
    pub client: u16,
    pub amount: i64,
    pub dispute_state: DisputeState,
}

#[derive(Debug, Default)]
pub struct Account {
    pub available: i64,
    pub held: i64,
    pub locked: bool,
}

impl Account {
    pub fn total(&self) -> i64 {
        self.available + self.held
    }
}

#[derive(Debug, Serialize)]
pub struct AccountOutput {
    pub client: u16,
    #[serde(serialize_with = "serialize_fixed")]
    pub available: i64,
    #[serde(serialize_with = "serialize_fixed")]
    pub held: i64,
    #[serde(serialize_with = "serialize_fixed")]
    pub total: i64,
    pub locked: bool,
}
