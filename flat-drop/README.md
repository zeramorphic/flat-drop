In this crate, we define the `FlatDrop` type.
`FlatDrop<K>` behaves just like a `K`, but with a custom `Drop` implementation
that avoids blowing up the stack when dropping large objects.
Instead of recursively dropping subobjects, we perform a depth-first search
and iteratively drop subobjects.

To use this crate, you can replace recursive `Box`es and `Arc`s in your types
with `FlatDrop<Box<T>>` or `FlatDrop<Arc<T>>`. You'll need to implement the
`Recursive` trait for your type, which performs one step of the iterative
dropping procedure.

This crate uses `unsafe` internally, but the external API is safe.

# Example

```rs
use flat_drop::{FlatDrop, Recursive};

/// Peano natural numbers.
enum Natural {
    Zero,
    Succ(FlatDrop<Box<Natural>>),
}

impl Recursive for Natural {
    type Container = Box<Natural>;

    fn destruct(self) -> impl Iterator<Item = Self::Container> {
        match self {
            Natural::Zero => None,
            Natural::Succ(pred) => Some(pred.into_inner()),
        }
        .into_iter()
    }
}

impl Natural {
    pub fn from_usize(value: usize) -> Self {
        (0..value).fold(Self::Zero, |nat, _| {
            Self::Succ(FlatDrop::new(Box::new(nat)))
        })
    }
}

// Create a new thread with a 4kb stack and allocate a number far bigger than 4 * 1024.
const STACK_SIZE: usize = 4 * 1024;

fn task() {
    let nat = Natural::from_usize(STACK_SIZE * 100);
    drop(std::hint::black_box(nat));
}

std::thread::Builder::new()
    .stack_size(STACK_SIZE)
    .spawn(task)
    .unwrap()
    .join()
    .unwrap();
```
