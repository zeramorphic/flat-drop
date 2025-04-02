use std::{
    borrow::{Borrow, BorrowMut},
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

/// The [Recursive::destruct] function decomposes an object into some component parts.
/// Usually, [Recursive::Output] is something like `Box<Self>` or `Arc<Self>`.
pub trait Recursive {
    type Container;

    fn destruct(self) -> impl Iterator<Item = Self::Container>;
}

/// A trait for a smart pointer that contains (at most) a single value.
pub trait IntoOptionInner {
    type Inner;

    /// A (potentially) fallible operation to convert the container into its internal value.
    /// This should never drop any data.
    ///
    /// If `Self == Box`, this will always return `Some(*self)`.
    /// If `Self == Arc`, this is `Arc::into_inner`.
    fn into_option_inner(self) -> Option<Self::Inner>;
}

/// If `K` is a container of a recursive type, such as `Box<T>` where `T: Recursive`,
/// `FlatDrop<K>` behaves just like `K`, but with a custom `Drop` implementation.
/// In this implementation, we gather the recursive parts of the object iteratively
/// and drop them without recursion, avoiding stack overflows when dropping
/// large recursive objects.
///
/// # Safety
///
/// We keep the invariant that the inner object is always initialised, but will
/// be dropped (exactly once) in the `drop` implementation.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct FlatDrop<K>(ManuallyDrop<K>)
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>;

impl<K> Drop for FlatDrop<K>
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
{
    fn drop(&mut self) {
        // Move out of the inner `ManuallyDrop`.
        // Safety: the inner value has not yet been dropped, and will not be used again.
        let value = unsafe { ManuallyDrop::take(&mut self.0) };

        // Construct a sequence of containers to drop.
        let mut to_drop = vec![value];

        // Iteratively decompose each container from this list.
        // This avoids creating excessive stack frames when destroying large objects.
        while let Some(container) = to_drop.pop() {
            if let Some(value) = container.into_option_inner() {
                to_drop.extend(value.destruct());
            }
        }

        // The drop glue will be a no-op since the field is `ManuallyDrop`.
    }
}

// Now that we've defined the core parts of the library, we'll make some API.

impl<T> IntoOptionInner for Box<T> {
    type Inner = T;

    fn into_option_inner(self) -> Option<Self::Inner> {
        Some(*self)
    }
}

impl<T> IntoOptionInner for Rc<T> {
    type Inner = T;

    fn into_option_inner(self) -> Option<Self::Inner> {
        Rc::into_inner(self)
    }
}

impl<T> IntoOptionInner for Arc<T> {
    type Inner = T;

    fn into_option_inner(self) -> Option<Self::Inner> {
        Arc::into_inner(self)
    }
}

impl<K> FlatDrop<K>
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
{
    pub const fn new(container: K) -> Self {
        Self(ManuallyDrop::new(container))
    }

    pub fn into_inner(mut self) -> K {
        // Safety: This value is always initialised.
        // Once we take it, we need to be careful to not call `drop` on `self`.
        let value = unsafe { ManuallyDrop::take(&mut self.0) };
        // This doesn't leak, because `self` is contained purely on the stack.
        std::mem::forget(self);
        value
    }
}

impl<K, T> AsRef<T> for FlatDrop<K>
where
    T: ?Sized,
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
    K: AsRef<T>,
{
    fn as_ref(&self) -> &T {
        (**self).as_ref()
    }
}

impl<K, T> AsMut<T> for FlatDrop<K>
where
    T: ?Sized,
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
    K: AsMut<T>,
{
    fn as_mut(&mut self) -> &mut T {
        (**self).as_mut()
    }
}

impl<K> Deref for FlatDrop<K>
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
{
    type Target = K;

    fn deref(&self) -> &K {
        self.0.deref()
    }
}

impl<K> DerefMut for FlatDrop<K>
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
{
    fn deref_mut(&mut self) -> &mut K {
        self.0.deref_mut()
    }
}

impl<K> From<K> for FlatDrop<K>
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
{
    fn from(value: K) -> Self {
        Self::new(value)
    }
}

impl<T> FlatDrop<Box<T>>
where
    T: Recursive<Container = Box<T>>,
{
    pub fn new_boxed(value: T) -> Self {
        Self::new(Box::new(value))
    }
}

impl<T> FlatDrop<Rc<T>>
where
    T: Recursive<Container = Rc<T>>,
{
    pub fn new_rc(value: T) -> Self {
        Self::new(Rc::new(value))
    }
}

impl<T> FlatDrop<Arc<T>>
where
    T: Recursive<Container = Arc<T>>,
{
    pub fn new_arc(value: T) -> Self {
        Self::new(Arc::new(value))
    }
}

#[cfg(feature = "serde")]
impl<K> serde::Serialize for FlatDrop<K>
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
    K: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        <K as serde::Serialize>::serialize(self, serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, K> serde::Deserialize<'de> for FlatDrop<K>
where
    K: IntoOptionInner,
    K::Inner: Recursive<Container = K>,
    K: serde::Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <K as serde::Deserialize>::deserialize(deserializer).map(Self::new)
    }
}

#[cfg(test)]
mod tests {
    use crate::{FlatDrop, Recursive};

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

    #[test]
    fn test_large_natural() {
        // Create a new thread with a 4kb stack and allocate a number far bigger than 4 * 1024.
        const STACK_SIZE: usize = 4 * 1024;

        fn task() {
            let nat = Natural::from_usize(STACK_SIZE * 100);
            println!("Dropping...");
            drop(std::hint::black_box(nat));
            println!("Dropped.");
        }

        std::thread::Builder::new()
            .stack_size(STACK_SIZE)
            .spawn(task)
            .unwrap()
            .join()
            .unwrap();
    }
}
