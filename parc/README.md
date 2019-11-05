# *parc* [![Latest Version](https://img.shields.io/crates/v/parc.svg)](https://crates.io/crates/parc) [![Rust Documentation](https://docs.rs/parc/badge.svg)](https://docs.rs/parc)

This crate exposes `ParentArc<T>` which is comparable to an `Arc<T>` but "strong" references cannot be cloned. This allows the `ParentArc<T>` to lock its weak references and block until all strong references are dropped. Once it is the only reference it can be consummed safely.

## Usage

This crate is compatible with `#![no_std]` environnement that provides an allocator.

```toml
[dependencies]
parc = {version="1", default-features=false} # for no_std
```


## Example

```rust
use parc::ParentArc;
use std::thread;
use std::sync;

fn main() {
	let m = ParentArc::new(sync::Mutex::new(0));

	let mut vh = Vec::new();
	for _ in 0..10 {
		let h = thread::spawn({
			let weak = ParentArc::downgrade(&m);
			move || loop {
				match weak.upgrade() {
					Some(mutex) => *mutex.lock().unwrap() += 1,
					None => break,
				}
			}
		});
		vh.push(h);
	}

	let _: sync::Mutex<usize> = m.block_into_inner();
	for h in vh {
		let _ = h.join();
	}
}
```
