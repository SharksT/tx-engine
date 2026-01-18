use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::types::{to_fixed, Account, AccountOutput, DisputeState, StoredTransaction, Transaction, TransactionType};

pub struct Engine {
    accounts: HashMap<u16, Account>,
    transactions: HashMap<u32, StoredTransaction>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            transactions: HashMap::new(),
        }
    }

    pub fn process(&mut self, tx: Transaction) {
        match tx.tx_type {
            TransactionType::Deposit => self.deposit(tx),
            TransactionType::Withdrawal => self.withdrawal(tx),
            TransactionType::Dispute => self.dispute(tx),
            TransactionType::Resolve => self.resolve(tx),
            TransactionType::Chargeback => self.chargeback(tx),
        }
    }

    fn deposit(&mut self, tx: Transaction) {
        let Some(decimal_amount) = tx.amount else { return };
        if decimal_amount <= Decimal::ZERO {
            return;
        }

        let amount = to_fixed(decimal_amount);

        let account = self.accounts.entry(tx.client).or_default();
        if account.locked {
            return;
        }

        account.available = account.available.saturating_add(amount);

        self.transactions.insert(
            tx.tx,
            StoredTransaction {
                client: tx.client,
                amount,
                dispute_state: DisputeState::None,
            },
        );
    }

    fn withdrawal(&mut self, tx: Transaction) {
        let Some(decimal_amount) = tx.amount else { return };
        if decimal_amount <= Decimal::ZERO {
            return;
        }

        let amount = to_fixed(decimal_amount);

        let account = self.accounts.entry(tx.client).or_default();
        if account.locked {
            return;
        }

        if account.available >= amount {
            account.available = account.available.saturating_sub(amount);
        }
    }

    /// Only deposits are stored, so disputes implicitly only apply to deposits.
    /// Disputes can still happen if the account is locked.
    /// A transaction can only be disputed if it's not currently disputed and hasn't been chargedback.
    fn dispute(&mut self, tx: Transaction) {
        let Some(stored) = self.transactions.get_mut(&tx.tx) else {
            return;
        };

        if stored.client != tx.client || stored.dispute_state != DisputeState::None {
            return;
        }

        let account = self.accounts.entry(tx.client).or_default();

        stored.dispute_state = DisputeState::Disputed;
        account.available = account.available.saturating_sub(stored.amount);
        account.held = account.held.saturating_add(stored.amount);
    }

    /// Resolve returns held funds to available. Only works on currently disputed transactions.
    /// After resolve, the transaction returns to None state and can be disputed again.
    fn resolve(&mut self, tx: Transaction) {
        let Some(stored) = self.transactions.get_mut(&tx.tx) else {
            return;
        };

        if stored.client != tx.client || stored.dispute_state != DisputeState::Disputed {
            return;
        }

        let account = self.accounts.entry(tx.client).or_default();

        stored.dispute_state = DisputeState::None;
        account.held = account.held.saturating_sub(stored.amount);
        account.available = account.available.saturating_add(stored.amount);
    }

    /// Chargeback is a terminal state - the transaction can never be disputed again.
    fn chargeback(&mut self, tx: Transaction) {
        let Some(stored) = self.transactions.get_mut(&tx.tx) else {
            return;
        };

        if stored.client != tx.client || stored.dispute_state != DisputeState::Disputed {
            return;
        }

        let account = self.accounts.entry(tx.client).or_default();

        stored.dispute_state = DisputeState::ChargedBack;
        account.held = account.held.saturating_sub(stored.amount);
        account.locked = true;
    }

    pub fn output(&self) -> Vec<AccountOutput> {
        self.accounts
            .iter()
            .map(|(&client, account)| AccountOutput {
                client,
                available: account.available,
                held: account.held,
                total: account.total(),
                locked: account.locked,
            })
            .collect()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SCALE;
    use rust_decimal_macros::dec;

    fn deposit(client: u16, tx: u32, amount: Decimal) -> Transaction {
        Transaction {
            tx_type: TransactionType::Deposit,
            client,
            tx,
            amount: Some(amount),
        }
    }

    fn withdrawal(client: u16, tx: u32, amount: Decimal) -> Transaction {
        Transaction {
            tx_type: TransactionType::Withdrawal,
            client,
            tx,
            amount: Some(amount),
        }
    }

    fn dispute(client: u16, tx: u32) -> Transaction {
        Transaction {
            tx_type: TransactionType::Dispute,
            client,
            tx,
            amount: None,
        }
    }

    fn resolve(client: u16, tx: u32) -> Transaction {
        Transaction {
            tx_type: TransactionType::Resolve,
            client,
            tx,
            amount: None,
        }
    }

    fn chargeback(client: u16, tx: u32) -> Transaction {
        Transaction {
            tx_type: TransactionType::Chargeback,
            client,
            tx,
            amount: None,
        }
    }

    /// Helper to create fixed-point value from integer and decimal parts
    fn fixed(whole: i64, frac: i64) -> i64 {
        whole * SCALE + frac
    }

    #[test]
    fn test_deposit() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
        assert_eq!(account.held, 0);
        assert_eq!(account.total, fixed(10, 0));
        assert!(!account.locked);
    }

    #[test]
    fn test_multiple_deposits() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(deposit(1, 2, dec!(5.5)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(15, 5000));
    }

    #[test]
    fn test_withdrawal_sufficient_funds() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(withdrawal(1, 2, dec!(4.0)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(6, 0));
    }

    #[test]
    fn test_withdrawal_insufficient_funds() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(withdrawal(1, 2, dec!(15.0)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
    }

    #[test]
    fn test_withdrawal_exact_balance() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(withdrawal(1, 2, dec!(10.0)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
    }

    #[test]
    fn test_dispute() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
        assert_eq!(account.held, fixed(10, 0));
        assert_eq!(account.total, fixed(10, 0));
    }

    #[test]
    fn test_dispute_nonexistent_tx() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 999));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
        assert_eq!(account.held, 0);
    }

    #[test]
    fn test_dispute_wrong_client() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(2, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
    }

    #[test]
    fn test_double_dispute_ignored() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 1));
        engine.process(dispute(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
        assert_eq!(account.held, fixed(10, 0));
    }

    #[test]
    fn test_resolve() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 1));
        engine.process(resolve(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
        assert_eq!(account.held, 0);
        assert!(!account.locked);
    }

    #[test]
    fn test_resolve_not_disputed() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(resolve(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
    }

    #[test]
    fn test_chargeback() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 1));
        engine.process(chargeback(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
        assert_eq!(account.held, 0);
        assert_eq!(account.total, 0);
        assert!(account.locked);
    }

    #[test]
    fn test_chargeback_not_disputed() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(chargeback(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
        assert!(!account.locked);
    }

    #[test]
    fn test_locked_account_rejects_deposit() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 1));
        engine.process(chargeback(1, 1));
        engine.process(deposit(1, 2, dec!(50.0)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
        assert!(account.locked);
    }

    #[test]
    fn test_locked_account_rejects_withdrawal() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(deposit(1, 2, dec!(10.0)));
        engine.process(dispute(1, 1));
        engine.process(chargeback(1, 1));
        engine.process(withdrawal(1, 3, dec!(5.0)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(10, 0));
    }

    #[test]
    fn test_locked_account_allows_dispute() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(deposit(1, 2, dec!(20.0)));
        engine.process(dispute(1, 1));
        engine.process(chargeback(1, 1));
        // Account is now locked with 20 available
        engine.process(dispute(1, 2)); // Should still work

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
        assert_eq!(account.held, fixed(20, 0));
        assert!(account.locked);
    }

    #[test]
    fn test_locked_account_allows_resolve() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(deposit(1, 2, dec!(20.0)));
        engine.process(dispute(1, 2)); // Dispute tx 2 first
        engine.process(dispute(1, 1));
        engine.process(chargeback(1, 1)); // Lock via tx 1
        // Account is now locked with 0 available, 20 held
        engine.process(resolve(1, 2)); // Should still work

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(20, 0));
        assert_eq!(account.held, 0);
        assert!(account.locked);
    }

    #[test]
    fn test_dispute_withdrawal_ignored() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(withdrawal(1, 2, dec!(5.0)));
        engine.process(dispute(1, 2));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(5, 0));
        assert_eq!(account.held, 0);
    }

    #[test]
    fn test_chargeback_prevents_redispute() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 1));
        engine.process(chargeback(1, 1));
        // Try to dispute again - should be ignored
        engine.process(dispute(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
        assert_eq!(account.held, 0); // Should still be 0, not 10
        assert!(account.locked);
    }

    #[test]
    fn test_resolve_allows_redispute() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(dispute(1, 1));
        engine.process(resolve(1, 1));
        // Dispute again after resolve - should work
        engine.process(dispute(1, 1));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, 0);
        assert_eq!(account.held, fixed(10, 0));
    }

    #[test]
    fn test_precision() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(1.2345)));
        engine.process(deposit(1, 2, dec!(0.0001)));

        let output = engine.output();
        let account = output.iter().find(|a| a.client == 1).unwrap();
        assert_eq!(account.available, fixed(1, 2346));
    }

    #[test]
    fn test_multiple_clients() {
        let mut engine = Engine::new();
        engine.process(deposit(1, 1, dec!(10.0)));
        engine.process(deposit(2, 2, dec!(20.0)));
        engine.process(withdrawal(1, 3, dec!(5.0)));

        let output = engine.output();
        let client1 = output.iter().find(|a| a.client == 1).unwrap();
        let client2 = output.iter().find(|a| a.client == 2).unwrap();
        assert_eq!(client1.available, fixed(5, 0));
        assert_eq!(client2.available, fixed(20, 0));
    }
}
