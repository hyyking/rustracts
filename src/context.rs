pub trait ContractContext {
    fn is_valid(&self) -> bool {
        true
    }
}

impl ContractContext for bool {
    fn is_valid(&self) -> bool {
        *self
    }
}

impl ContractContext for u8 {}
impl ContractContext for u16 {}
impl ContractContext for u32 {}
impl ContractContext for u64 {}
impl ContractContext for u128 {}
impl ContractContext for usize {}

impl ContractContext for i8 {}
impl ContractContext for i16 {}
impl ContractContext for i32 {}
impl ContractContext for i64 {}
impl ContractContext for i128 {}
impl ContractContext for isize {}

/// Empty context to use if you want non-voidable contracts
#[derive(Copy, Clone)]
pub struct DefaultContext;

impl ContractContext for DefaultContext {}

impl Default for DefaultContext {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultContext {
    pub fn new() -> Self {
        Self
    }
}

/// Common compare contexes
pub mod cmp {
    use super::ContractContext;

    /// Context to compare the equality of two elements
    pub struct EqContext<A, B>(pub A, pub B);

    impl<A, B> ContractContext for EqContext<A, B>
    where
        A: PartialEq<B>,
    {
        fn is_valid(&self) -> bool {
            self.0 == self.1
        }
    }

    /// Context to compare the inequality of two elements
    #[derive(Copy, Clone)]
    pub struct NqContext<A, B>(pub A, pub B);

    impl<A, B> ContractContext for NqContext<A, B>
    where
        A: PartialEq<B>,
    {
        fn is_valid(&self) -> bool {
            self.0 != self.1
        }
    }

    /// Context to compare the less than ordering of two elements
    #[derive(Copy, Clone)]
    pub struct LtContext<A>(pub A, pub A);

    impl<A> ContractContext for LtContext<A>
    where
        A: Ord,
    {
        fn is_valid(&self) -> bool {
            self.0 < self.1
        }
    }

    /// Context to compare the less or equal ordering of two elements
    #[derive(Copy, Clone)]
    pub struct LeContext<A>(pub A, pub A);

    impl<A> ContractContext for LeContext<A>
    where
        A: Ord,
    {
        fn is_valid(&self) -> bool {
            self.0 <= self.1
        }
    }

    /// Context to compare the greater than ordering of two elements
    #[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
    pub struct GtContext<A>(pub A, pub A);

    impl<A> ContractContext for GtContext<A>
    where
        A: Ord,
    {
        fn is_valid(&self) -> bool {
            self.0 > self.1
        }
    }

    /// Context to compare the greater or equal ordering of two elements
    #[derive(Copy, Clone)]
    pub struct GeContext<A>(pub A, pub A);

    impl<A> ContractContext for GeContext<A>
    where
        A: Ord,
    {
        fn is_valid(&self) -> bool {
            self.0 > self.1
        }
    }
}
