# `dynsequence`

[![Crates.io](https://img.shields.io/crates/v/dynsequence.svg?label=dynsequence)](https://crates.io/crates/dynsequence)
[![docs.rs](https://docs.rs/dynsequence/badge.svg)](https://docs.rs/dynsequence/)
[![license: MIT/Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](#license)
[![Rust CI](https://github.com/HellButcher/dynsequence-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/HellButcher/dynsequence-rs/actions/workflows/rust.yml)

<!-- Short Introduction -->

`DynSequence<dyn Trait>` is like `Vec<Box<dyn Trait>>`, but with an optimization that avoids allocations. This works by using multiple larger blocks of memory and storing their pointers in a `Vec`. This means, the items are randomly accessible, but may not lay in continues memory.

## Example

This example stores multiple values the `DynSequence` and accesses them.
(`push(...)` requires the `"unstable"` feature (**nightly only**))

```rust
# #[cfg(feature="unstable")] {
use dynsequence::DynSequence;
use std::any::Any;
let mut seq: DynSequence<dyn Any> = DynSequence::new();
seq.push("foo"); 
seq.push(1234);
assert_eq!(Some(&1234), seq.get(1).and_then(|a| a.downcast_ref()));
assert_eq!(Some(&"foo"), seq.get(0).and_then(|a| a.downcast_ref()));
assert!(seq.get(2).is_none());
assert_eq!(None, seq.get(0).and_then(|a| a.downcast_ref::<bool>()));
# }
```

The following example shows the usage of a macro-hac that also works on `stable`


```rust
use dynsequence::{DynSequence,dyn_sequence};
use std::any::Any;
// construct with macro hack
let mut seq: DynSequence<dyn Any> = dyn_sequence![dyn Any => "foo", 1234];
assert_eq!(Some(&1234), seq.get(1).and_then(|a| a.downcast_ref()));
assert_eq!(Some(&"foo"), seq.get(0).and_then(|a| a.downcast_ref()));
assert!(seq.get(2).is_none());

// push with macro hack
dyn_sequence![dyn Any | &mut seq => {
  push (true);
} ];
assert_eq!(Some(&true), seq.get(2).and_then(|a| a.downcast_ref()));

```

## `no_std`

This crate should also work without `std` (with `alloc`). No additional configuration required.

## License

[license]: #license

This repository is licensed under either of

- MIT license ([LICENSE-MIT] or <http://opensource.org/licenses/MIT>)
- Apache License, Version 2.0, ([LICENSE-APACHE] or <http://www.apache.org/licenses/LICENSE-2.0>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

[license-mit]: ./LICENSE-MIT
[license-apache]: ./LICENSE-APACHE
