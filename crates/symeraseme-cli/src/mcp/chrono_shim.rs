#![allow(dead_code)]
//! Minimal compatibility names for the immutable frozen-reader helper in
//! `rust-tests/dashboard_compact.rs`. The helper's `use chrono::{DateTime, Utc}`
//! is resolved through an `extern crate self as chrono` alias at each crate
//! root, so the recorded helper itself stays byte-for-byte unchanged.

use std::cmp::Ordering;
use std::marker::PhantomData;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Clone, Copy, Debug)]
pub struct Utc;

#[derive(Clone, Copy, Debug)]
pub struct DateTime<Tz = Utc> {
    inner: OffsetDateTime,
    zone: PhantomData<Tz>,
}

impl<Tz> DateTime<Tz> {
    pub fn from_utc(value: OffsetDateTime) -> Self {
        Self {
            inner: value,
            zone: PhantomData,
        }
    }
}

impl DateTime<Utc> {
    pub fn parse_from_rfc3339(value: &str) -> Result<Self, time::error::Parse> {
        OffsetDateTime::parse(value, &Rfc3339).map(|inner| Self {
            inner,
            zone: PhantomData,
        })
    }
}

impl<Tz1, Tz2> PartialEq<DateTime<Tz2>> for DateTime<Tz1> {
    fn eq(&self, other: &DateTime<Tz2>) -> bool {
        self.inner == other.inner
    }
}

impl<Tz> Eq for DateTime<Tz> {}

impl<Tz1, Tz2> PartialOrd<DateTime<Tz2>> for DateTime<Tz1> {
    fn partial_cmp(&self, other: &DateTime<Tz2>) -> Option<Ordering> {
        self.inner.partial_cmp(&other.inner)
    }
}
