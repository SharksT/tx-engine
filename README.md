# tx-engine

A toy transaction engine that processes payments, handling deposits, withdrawals, and dispute resolution for client accounts.

## Usage

```bash
cargo build --release
cargo run --release -- transactions.csv > accounts.csv
```

### Input

CSV file with columns: `type`, `client`, `tx`, `amount`

```csv
type, client, tx, amount
deposit, 1, 1, 1.0
deposit, 2, 2, 2.0
deposit, 1, 3, 2.0
withdrawal, 1, 4, 1.5
withdrawal, 2, 5, 3.0
```

### Output

CSV to stdout with columns: `client`, `available`, `held`, `total`, `locked`

```csv
client,available,held,total,locked
1,1.5000,0.0000,1.5000,false
2,2.0000,0.0000,2.0000,false
```

Client 1: deposits 1.0 + 2.0 = 3.0, withdraws 1.5 → 1.5 available
Client 2: deposits 2.0, withdrawal of 3.0 fails (insufficient funds) → 2.0 available

## Transaction Types

| Type | Effect |
|------|--------|
| `deposit` | Credits available funds |
| `withdrawal` | Debits available funds (fails silently if insufficient) |
| `dispute` | Moves disputed deposit amount from available to held |
| `resolve` | Releases held funds back to available |
| `chargeback` | Removes held funds, freezes account |

## Architecture

```
src/
├── lib.rs      # Public exports
├── types.rs    # Data structures
├── engine.rs   # Core logic + tests
└── main.rs     # CLI
```

The engine processes transactions sequentially via a streaming CSV reader - records are parsed and processed one at a time without loading the entire file into memory.

Two HashMaps track state:
- `accounts` - keyed by client ID (u16)
- `transactions` - keyed by tx ID (u32), storing only deposits

Withdrawals are not stored - they only affect the account balance at processing time and cannot be disputed. This reduces memory usage since only deposits need to be retained for potential dispute resolution.

## Design Decisions

**Disputes only apply to deposits.** A dispute moves funds from available to held. For withdrawals, the funds have already left the account, so this operation doesn't apply.

**Negative available balances are possible.** If a client deposits funds, withdraws some, and then the deposit is disputed, the available balance can go negative. This mirrors real banking behavior - a cleared check can be reversed even after funds are spent, leaving the account overdrawn. The negative balance represents a debt owed by the client.

**Frozen accounts can still have disputes processed.** When an account is locked (after a chargeback), new deposits and withdrawals are blocked. However, disputes and resolves on past transactions are still allowed - a frozen account shouldn't prevent investigation of potentially fraudulent transactions.

**Zero and negative amounts are ignored.** Deposits and withdrawals with amounts <= 0 are silently skipped. Zero-amount transactions have no effect and would waste memory if stored.

**Invalid input terminates processing.** Malformed CSV rows cause the program to exit with an error rather than silently skipping. This ensures data integrity at the cost of fault tolerance.

**Fixed-point i64 arithmetic for memory efficiency.** Amounts are stored as `i64` with 4 decimal places of precision (value * 10,000). This uses 8 bytes per amount versus 16 bytes for `Decimal`, reducing memory usage by ~33% for stored transactions. The `rust_decimal` crate is still used for parsing input, then converted to fixed-point for storage and arithmetic. The i64 range supports amounts up to ~922 trillion, far exceeding practical transaction values.

**Panic-free arithmetic.** All arithmetic operations use saturating functions (`saturating_add`, `saturating_sub`) that clamp at `i64::MAX/MIN` instead of panicking on overflow or wrapping to incorrect values. Output formatting uses `wrapping_abs()` to safely handle edge cases like `i64::MIN`. This ensures the engine never panics due to arithmetic, even with extreme input.

## Testing

```bash
cargo test
```

20 unit tests cover:
- Deposit and withdrawal operations
- Insufficient funds handling
- Dispute lifecycle (dispute → resolve, dispute → chargeback)
- Edge cases (nonexistent tx, wrong client, double dispute)
- Locked account behavior
- Decimal precision

## Limitations

- **No persistence**: All state is in-memory. For large datasets exceeding available memory, transactions would need to be persisted to disk or database.

- **Single-threaded**: Transactions are processed sequentially. For this use case (single CSV file), the bottleneck is I/O and parsing, not processing - parallelism would add overhead without meaningful speedup. For scenarios with multiple concurrent streams (e.g., thousands of TCP connections), the CLI model naturally scales by running multiple processes in parallel - each with its own isolated memory space, no shared state, and no locking complexity.

- **Fail-fast on bad input**: A single malformed row stops processing. This is intentional - a malformed transaction type like "depositt" is likely a typo for "deposit". Silently skipping it would result in missing funds and an invalid final state. Failing fast ensures data integrity by surfacing errors immediately rather than producing incorrect output.
