// SPDX-License-Identifier: Apache-2.0

//! Compile-time configuration validation
//!
//! This module contains compile-time checks to ensure that mutually exclusive
//! features are not enabled simultaneously.

// Compile-time checks for mutually exclusive integer width features
#[cfg(all(feature = "int32", feature = "int64"))]
compile_error!(
    "Cannot enable both 'int32' and 'int64' features simultaneously: choose one integer width"
);

// Compile-time checks for mutually exclusive float behavior features
#[cfg(all(feature = "float-skip", feature = "float-error"))]
compile_error!("Cannot enable both 'float-skip' and 'float-error' features simultaneously");

#[cfg(all(feature = "float-skip", feature = "float-truncate"))]
compile_error!("Cannot enable both 'float-skip' and 'float-truncate' features simultaneously");

#[cfg(all(feature = "float-error", feature = "float-truncate"))]
compile_error!("Cannot enable both 'float-error' and 'float-truncate' features simultaneously");

#[cfg(all(
    feature = "float-skip",
    feature = "float-error",
    feature = "float-truncate"
))]
compile_error!("Cannot enable multiple float behavior features: choose only one of 'float-skip', 'float-error', or 'float-truncate'");

// Compile-time checks to prevent 'float' feature conflicts with float-behavior features
#[cfg(all(feature = "float", feature = "float-skip"))]
compile_error!("Cannot enable both 'float' and 'float-skip' features: 'float-skip' is only for when float parsing is disabled");

#[cfg(all(feature = "float", feature = "float-error"))]
compile_error!("Cannot enable both 'float' and 'float-error' features: 'float-error' is only for when float parsing is disabled");

#[cfg(all(feature = "float", feature = "float-truncate"))]
compile_error!("Cannot enable both 'float' and 'float-truncate' features: 'float-truncate' is only for when float parsing is disabled");
