use core::error::Error;
use core::{mem, fmt::{self, Debug, Display}};

use crate::{boxed::Box, string::String, borrow::Cow};

#[stable(feature = "box_error", since = "1.8.0")]
impl<T: Error> Error for Box<T> {
    #[allow(deprecated, deprecated_in_future)]
    fn description(&self) -> &str {
        Error::description(&**self)
    }

    #[allow(deprecated)]
    fn cause(&self) -> Option<&dyn Error> {
        Error::cause(&**self)
    }

    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Error::source(&**self)
    }
}

impl Box<dyn Error> {
    #[inline]
    #[stable(feature = "error_downcast", since = "1.3.0")]
    /// Attempts to downcast the box to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<Box<T>, Box<dyn Error>> {
        if self.is::<T>() {
            unsafe {
                let raw: *mut dyn Error = Box::into_raw(self);
                Ok(Box::from_raw(raw as *mut T))
            }
        } else {
            Err(self)
        }
    }
}

impl Box<dyn Error + Send> {
    #[inline]
    #[stable(feature = "error_downcast", since = "1.3.0")]
    /// Attempts to downcast the box to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<Box<T>, Self> {
        let err: Box<dyn Error> = self;
        <Box<dyn Error>>::downcast(err).map_err(|s| unsafe {
            // Reapply the `Send` marker.
            mem::transmute::<Box<dyn Error>, Box<dyn Error + Send>>(s)
        })
    }
}

impl Box<dyn Error + Send + Sync> {
    #[inline]
    #[stable(feature = "error_downcast", since = "1.3.0")]
    /// Attempts to downcast the box to a concrete type.
    pub fn downcast<T: Error + 'static>(self) -> Result<Box<T>, Self> {
        let err: Box<dyn Error> = self;
        <Box<dyn Error>>::downcast(err).map_err(|s| unsafe {
            // Reapply the `Send + Sync` marker.
            mem::transmute::<Box<dyn Error>, Box<dyn Error + Send + Sync>>(s)
        })
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<'a, E: Error + Send + Sync + 'a> From<E> for Box<dyn Error + Send + Sync + 'a> {
    /// Converts a type of [`Error`] + [`Send`] + [`Sync`] into a box of
    /// dyn [`Error`] + [`Send`] + [`Sync`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::error::Error;
    /// use std::fmt;
    /// use std::mem;
    ///
    /// #[derive(Debug)]
    /// struct AnError;
    ///
    /// impl fmt::Display for AnError {
    ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    ///         write!(f , "An error")
    ///     }
    /// }
    ///
    /// impl Error for AnError {}
    ///
    /// unsafe impl Send for AnError {}
    ///
    /// unsafe impl Sync for AnError {}
    ///
    /// let an_error = AnError;
    /// assert!(0 == mem::size_of_val(&an_error));
    /// let a_boxed_error = Box::<dyn Error + Send + Sync>::from(an_error);
    /// assert!(
    ///     mem::size_of::<Box<dyn Error + Send + Sync>>() == mem::size_of_val(&a_boxed_error))
    /// ```
    fn from(err: E) -> Box<dyn Error + Send + Sync + 'a> {
        Box::new(err)
    }
}

#[stable(feature = "cow_box_error", since = "1.22.0")]
impl<'a, 'b> From<Cow<'b, str>> for Box<dyn Error + Send + Sync + 'a> {
    /// Converts a [`Cow`] into a box of dyn [`Error`] + [`Send`] + [`Sync`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::error::Error;
    /// use std::mem;
    /// use std::borrow::Cow;
    ///
    /// let a_cow_str_error = Cow::from("a str error");
    /// let a_boxed_error = Box::<dyn Error + Send + Sync>::from(a_cow_str_error);
    /// assert!(
    ///     mem::size_of::<Box<dyn Error + Send + Sync>>() == mem::size_of_val(&a_boxed_error))
    /// ```
    fn from(err: Cow<'b, str>) -> Box<dyn Error + Send + Sync + 'a> {
        From::from(err.into_owned())
    }
}

#[stable(feature = "cow_box_error", since = "1.22.0")]
impl<'a> From<Cow<'a, str>> for Box<dyn Error> {
    /// Converts a [`Cow`] into a box of dyn [`Error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::error::Error;
    /// use std::mem;
    /// use std::borrow::Cow;
    ///
    /// let a_cow_str_error = Cow::from("a str error");
    /// let a_boxed_error = Box::<dyn Error>::from(a_cow_str_error);
    /// assert!(mem::size_of::<Box<dyn Error>>() == mem::size_of_val(&a_boxed_error))
    /// ```
    fn from(err: Cow<'a, str>) -> Box<dyn Error> {
        From::from(err.into_owned())
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl From<String> for Box<dyn Error + Send + Sync> {
    /// Converts a [`String`] into a box of dyn [`Error`] + [`Send`] + [`Sync`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::error::Error;
    /// use std::mem;
    ///
    /// let a_string_error = "a string error".to_string();
    /// let a_boxed_error = Box::<dyn Error + Send + Sync>::from(a_string_error);
    /// assert!(
    ///     mem::size_of::<Box<dyn Error + Send + Sync>>() == mem::size_of_val(&a_boxed_error))
    /// ```
    #[inline]
    fn from(err: String) -> Box<dyn Error + Send + Sync> {
        struct StringError(String);

        impl Error for StringError {
            #[allow(deprecated)]
            fn description(&self) -> &str {
                &self.0
            }
        }

        impl Display for StringError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        // Purposefully skip printing "StringError(..)"
        impl Debug for StringError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                Debug::fmt(&self.0, f)
            }
        }

        Box::new(StringError(err))
    }
}

#[stable(feature = "string_box_error", since = "1.6.0")]
impl From<String> for Box<dyn Error> {
    /// Converts a [`String`] into a box of dyn [`Error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::error::Error;
    /// use std::mem;
    ///
    /// let a_string_error = "a string error".to_string();
    /// let a_boxed_error = Box::<dyn Error>::from(a_string_error);
    /// assert!(mem::size_of::<Box<dyn Error>>() == mem::size_of_val(&a_boxed_error))
    /// ```
    fn from(str_err: String) -> Box<dyn Error> {
        let err1: Box<dyn Error + Send + Sync> = From::from(str_err);
        let err2: Box<dyn Error> = err1;
        err2
    }
}

// #[stable(feature = "rust1", since = "1.0.0")]
// impl<'a> From<&str> for Box<dyn Error + Send + Sync + 'a> {
//     /// Converts a [`str`] into a box of dyn [`Error`] + [`Send`] + [`Sync`].
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// use std::error::Error;
//     /// use std::mem;
//     ///
//     /// let a_str_error = "a str error";
//     /// let a_boxed_error = Box::<dyn Error + Send + Sync>::from(a_str_error);
//     /// assert!(
//     ///     mem::size_of::<Box<dyn Error + Send + Sync>>() == mem::size_of_val(&a_boxed_error))
//     /// ```
//     #[inline]
//     fn from(err: &str) -> Box<dyn Error + Send + Sync + 'a> {
//         From::from(String::from(err))
//     }
// }
//
// #[stable(feature = "string_box_error", since = "1.6.0")]
// impl From<&str> for Box<dyn Error> {
//     /// Converts a [`str`] into a box of dyn [`Error`].
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// use std::error::Error;
//     /// use std::mem;
//     ///
//     /// let a_str_error = "a str error";
//     /// let a_boxed_error = Box::<dyn Error>::from(a_str_error);
//     /// assert!(mem::size_of::<Box<dyn Error>>() == mem::size_of_val(&a_boxed_error))
//     /// ```
//     fn from(err: &str) -> Box<dyn Error> {
//         From::from(String::from(err))
//     }
// }

// TODO: move to where Box is defn'd
#[stable(feature = "rust1", since = "1.0.0")]
impl<'a, E: Error + 'a> From<E> for Box<dyn Error + 'a> {
    /// Converts a type of [`Error`] into a box of dyn [`Error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::error::Error;
    /// use std::fmt;
    /// use std::mem;
    ///
    /// #[derive(Debug)]
    /// struct AnError;
    ///
    /// impl fmt::Display for AnError {
    ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    ///         write!(f , "An error")
    ///     }
    /// }
    ///
    /// impl Error for AnError {}
    ///
    /// let an_error = AnError;
    /// assert!(0 == mem::size_of_val(&an_error));
    /// let a_boxed_error = Box::<dyn Error>::from(an_error);
    /// assert!(mem::size_of::<Box<dyn Error>>() == mem::size_of_val(&a_boxed_error))
    /// ```
    fn from(err: E) -> Box<dyn Error + 'a> {
        Box::new(err)
    }
}

#[unstable(feature = "try_reserve", reason = "new API", issue = "48043")]
impl Error for alloc::collections::TryReserveError {}

#[stable(feature = "rust1", since = "1.0.0")]
impl Error for string::FromUtf8Error {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "invalid utf-8"
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl Error for string::FromUtf16Error {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "invalid utf-16"
    }
}
