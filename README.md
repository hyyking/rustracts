# *rustracts*
Rust crate for voidable insurance contracts over a context in async/await rust

```toml
rustracts = "0.0.1"
```

## Features

### Available

- FuturesContract: Will produce a value at the end of it's lifetime if the contract is not voided

### To Come

- OptionContract: produce value if the secondary context has realised
- OnKillContract: produce a value if the contract is killed

## Examples

```rust
use std::time::Duration;
    
use crate::context::cmp::GtContext;
use crate::{ContractExt, Status, FuturesContract};

fn simple_contract() {
	let context: usize = 3;
	let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize { con + 5 });

	if let Status::Completed(value) = futures::executor::block_on(c) {
		assert_eq!(value, 8)
	} else {
		assert!(false)
	}
}

fn voided_contract() {
	let context = GtContext(3, 2); // Context is true if self.0 > self.1

	let c = FuturesContract::new(Duration::from_secs(4), context, |con| -> usize {
		con.0 + 5
	});

	let handle = std::thread::spawn({
		let mcontext = c.get_context();
		move || {
			(*mcontext.lock().unwrap()).0 = 1; // Modify context before contract ends
		}
	});

	if let Status::Completed(val) = futures::executor::block_on(c) {
		assert_ne!(val, 1);
	} else {
		assert!(true); // Contract should be voided because updated value is 1 which is < 2
	}

	handle.join().unwrap();
}

fn updated_contract() {
	let context = GtContext(3, 2); // Context is valid if self.0 > self.1

	let c = FuturesContract::new(Duration::from_secs(1), context, |con| -> usize {
		con.0 + 5
	});

	let handle = std::thread::spawn({
		let mcontext = c.get_context();
		move || {
			(*mcontext.lock().unwrap()).0 += 2;
		}
	});

	if let Status::Completed(value) = futures::executor::block_on(c) {
		assert_eq!(value, 10);
	} else {
		assert!(false);
	}

	handle.join().unwrap();
}
```
