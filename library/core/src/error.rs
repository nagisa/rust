//! Traits for working with Errors.

#![stable(feature = "rust1", since = "1.0.0")]

// A note about crates and the facade:
//
// Originally, the `Error` trait was defined in libcore, and the impls
// were scattered about. However, coherence objected to this
// arrangement, because to create the blanket impls for `Box` required
// knowing that `&str: !Error`, and we have no means to deal with that
// sort of conflict just now. Therefore, for the time being, we have
// moved the `Error` trait into libstd. As we evolve a sol'n to the
// coherence challenge (e.g., specialization, neg impls, etc) we can
// reconsider what crate these items belong in.

use crate::array;
use crate::convert::Infallible;

use crate::alloc::{AllocErr, LayoutErr};
use crate::any::TypeId;
use crate::cell;
use crate::char;
use crate::fmt::{self, Debug, Display};
use crate::num;
use crate::str;

/// `Error` is a trait representing the basic expectations for error values,
/// i.e., values of type `E` in [`Result<T, E>`]. Errors must describe
/// themselves through the [`Display`] and [`Debug`] traits, and may provide
/// cause chain information:
///
/// The [`source`] method is generally used when errors cross "abstraction
/// boundaries". If one module must report an error that is caused by an error
/// from a lower-level module, it can allow access to that error via the
/// [`source`] method. This makes it possible for the high-level module to
/// provide its own errors while also revealing some of the implementation for
/// debugging via [`source`] chains.
///
/// [`Result<T, E>`]: Result
/// [`source`]: Error::source
#[stable(feature = "rust1", since = "1.0.0")]
pub trait Error: Debug + Display {
    /// The lower-level source of this error, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::error::Error;
    /// use std::fmt;
    ///
    /// #[derive(Debug)]
    /// struct SuperError {
    ///     side: SuperErrorSideKick,
    /// }
    ///
    /// impl fmt::Display for SuperError {
    ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    ///         write!(f, "SuperError is here!")
    ///     }
    /// }
    ///
    /// impl Error for SuperError {
    ///     fn source(&self) -> Option<&(dyn Error + 'static)> {
    ///         Some(&self.side)
    ///     }
    /// }
    ///
    /// #[derive(Debug)]
    /// struct SuperErrorSideKick;
    ///
    /// impl fmt::Display for SuperErrorSideKick {
    ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    ///         write!(f, "SuperErrorSideKick is here!")
    ///     }
    /// }
    ///
    /// impl Error for SuperErrorSideKick {}
    ///
    /// fn get_super_error() -> Result<(), SuperError> {
    ///     Err(SuperError { side: SuperErrorSideKick })
    /// }
    ///
    /// fn main() {
    ///     match get_super_error() {
    ///         Err(e) => {
    ///             println!("Error: {}", e);
    ///             println!("Caused by: {}", e.source().unwrap());
    ///         }
    ///         _ => println!("No error"),
    ///     }
    /// }
    /// ```
    #[stable(feature = "error_source", since = "1.30.0")]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }

    /// Gets the `TypeId` of `self`.
    #[doc(hidden)]
    #[unstable(
        feature = "error_type_id",
        reason = "this is memory-unsafe to override in user code",
        issue = "60784"
    )]
    fn type_id(&self, _: private::Internal) -> TypeId
    where
        Self: 'static,
    {
        TypeId::of::<Self>()
    }

    /// ```
    /// if let Err(e) = "xc".parse::<u32>() {
    ///     // Print `e` itself, no need for description().
    ///     eprintln!("Error: {}", e);
    /// }
    /// ```
    #[stable(feature = "rust1", since = "1.0.0")]
    #[rustc_deprecated(since = "1.42.0", reason = "use the Display impl or to_string()")]
    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }

    #[stable(feature = "rust1", since = "1.0.0")]
    #[rustc_deprecated(
        since = "1.33.0",
        reason = "replaced by Error::source, which can support downcasting"
    )]
    #[allow(missing_docs)]
    fn cause(&self) -> Option<&dyn Error> {
        self.source()
    }
}

mod private {
    // This is a hack to prevent `type_id` from being overridden by `Error`
    // implementations, since that can enable unsound downcasting.
    #[unstable(feature = "error_type_id", issue = "60784")]
    #[derive(Debug)]
    pub struct Internal;
}

#[unstable(feature = "never_type", issue = "35121")]
impl Error for ! {}

#[unstable(
    feature = "allocator_api",
    reason = "the precise API and guarantees it provides may be tweaked.",
    issue = "32838"
)]
impl Error for AllocErr {}

#[unstable(
    feature = "allocator_api",
    reason = "the precise API and guarantees it provides may be tweaked.",
    issue = "32838"
)]
impl Error for LayoutErr {}

#[stable(feature = "rust1", since = "1.0.0")]
impl Error for str::ParseBoolError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "failed to parse bool"
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl Error for str::Utf8Error {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "invalid utf-8: corrupt contents"
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl Error for num::ParseIntError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        self.__description()
    }
}

#[stable(feature = "try_from", since = "1.34.0")]
impl Error for num::TryFromIntError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        self.__description()
    }
}

#[stable(feature = "try_from", since = "1.34.0")]
impl Error for array::TryFromSliceError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        self.__description()
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl Error for num::ParseFloatError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        self.__description()
    }
}

#[stable(feature = "str_parse_error2", since = "1.8.0")]
impl Error for Infallible {
    fn description(&self) -> &str {
        match *self {}
    }
}

#[stable(feature = "decode_utf16", since = "1.9.0")]
impl Error for char::DecodeUtf16Error {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "unpaired surrogate found"
    }
}


#[stable(feature = "fmt_error", since = "1.11.0")]
impl Error for fmt::Error {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "an error occurred when formatting an argument"
    }
}

#[stable(feature = "try_borrow", since = "1.13.0")]
impl Error for cell::BorrowError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "already mutably borrowed"
    }
}

#[stable(feature = "try_borrow", since = "1.13.0")]
impl Error for cell::BorrowMutError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "already borrowed"
    }
}

#[stable(feature = "try_from", since = "1.34.0")]
impl Error for char::CharTryFromError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "converted integer out of range for `char`"
    }
}

#[stable(feature = "char_from_str", since = "1.20.0")]
impl Error for char::ParseCharError {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        self.__description()
    }
}

// Copied from `any.rs`.
impl dyn Error + 'static {
    /// Returns `true` if the boxed type is the same as `T`
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn is<T: Error + 'static>(&self) -> bool {
        // Get `TypeId` of the type this function is instantiated with.
        let t = TypeId::of::<T>();

        // Get `TypeId` of the type in the trait object.
        let boxed = self.type_id(private::Internal);

        // Compare both `TypeId`s on equality.
        t == boxed
    }

    /// Returns some reference to the boxed value if it is of type `T`, or
    /// `None` if it isn't.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(&*(self as *const dyn Error as *const T)) }
        } else {
            None
        }
    }

    /// Returns some mutable reference to the boxed value if it is of type `T`, or
    /// `None` if it isn't.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn downcast_mut<T: Error + 'static>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(&mut *(self as *mut dyn Error as *mut T)) }
        } else {
            None
        }
    }
}

impl dyn Error + 'static + Send {
    /// Forwards to the method defined on the type `dyn Error`.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn is<T: Error + 'static>(&self) -> bool {
        <dyn Error + 'static>::is::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        <dyn Error + 'static>::downcast_ref::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn downcast_mut<T: Error + 'static>(&mut self) -> Option<&mut T> {
        <dyn Error + 'static>::downcast_mut::<T>(self)
    }
}

impl dyn Error + 'static + Send + Sync {
    /// Forwards to the method defined on the type `dyn Error`.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn is<T: Error + 'static>(&self) -> bool {
        <dyn Error + 'static>::is::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn downcast_ref<T: Error + 'static>(&self) -> Option<&T> {
        <dyn Error + 'static>::downcast_ref::<T>(self)
    }

    /// Forwards to the method defined on the type `dyn Error`.
    #[stable(feature = "error_downcast", since = "1.3.0")]
    #[inline]
    pub fn downcast_mut<T: Error + 'static>(&mut self) -> Option<&mut T> {
        <dyn Error + 'static>::downcast_mut::<T>(self)
    }
}

impl dyn Error {
    /// Returns an iterator starting with the current error and continuing with
    /// recursively calling [`source`].
    ///
    /// If you want to omit the current error and only use its sources,
    /// use `skip(1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(error_iter)]
    /// use std::error::Error;
    /// use std::fmt;
    ///
    /// #[derive(Debug)]
    /// struct A;
    ///
    /// #[derive(Debug)]
    /// struct B(Option<Box<dyn Error + 'static>>);
    ///
    /// impl fmt::Display for A {
    ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    ///         write!(f, "A")
    ///     }
    /// }
    ///
    /// impl fmt::Display for B {
    ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    ///         write!(f, "B")
    ///     }
    /// }
    ///
    /// impl Error for A {}
    ///
    /// impl Error for B {
    ///     fn source(&self) -> Option<&(dyn Error + 'static)> {
    ///         self.0.as_ref().map(|e| e.as_ref())
    ///     }
    /// }
    ///
    /// let b = B(Some(Box::new(A)));
    ///
    /// // let err : Box<Error> = b.into(); // or
    /// let err = &b as &(dyn Error);
    ///
    /// let mut iter = err.chain();
    ///
    /// assert_eq!("B".to_string(), iter.next().unwrap().to_string());
    /// assert_eq!("A".to_string(), iter.next().unwrap().to_string());
    /// assert!(iter.next().is_none());
    /// assert!(iter.next().is_none());
    /// ```
    ///
    /// [`source`]: Error::source
    #[unstable(feature = "error_iter", issue = "58520")]
    #[inline]
    pub fn chain(&self) -> Chain<'_> {
        Chain { current: Some(self) }
    }
}

/// An iterator over an [`Error`] and its sources.
///
/// If you want to omit the initial error and only process
/// its sources, use `skip(1)`.
#[unstable(feature = "error_iter", issue = "58520")]
#[derive(Clone, Debug)]
pub struct Chain<'a> {
    current: Option<&'a (dyn Error + 'static)>,
}

#[unstable(feature = "error_iter", issue = "58520")]
impl<'a> Iterator for Chain<'a> {
    type Item = &'a (dyn Error + 'static);

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current;
        self.current = self.current.and_then(Error::source);
        current
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::fmt;

    #[derive(Debug, PartialEq)]
    struct A;
    #[derive(Debug, PartialEq)]
    struct B;

    impl fmt::Display for A {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "A")
        }
    }
    impl fmt::Display for B {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "B")
        }
    }

    impl Error for A {}
    impl Error for B {}

    #[test]
    fn downcasting() {
        let mut a = A;
        let a = &mut a as &mut (dyn Error + 'static);
        assert_eq!(a.downcast_ref::<A>(), Some(&A));
        assert_eq!(a.downcast_ref::<B>(), None);
        assert_eq!(a.downcast_mut::<A>(), Some(&mut A));
        assert_eq!(a.downcast_mut::<B>(), None);

        let a: Box<dyn Error> = Box::new(A);
        match a.downcast::<B>() {
            Ok(..) => panic!("expected error"),
            Err(e) => assert_eq!(*e.downcast::<A>().unwrap(), A),
        }
    }
}
