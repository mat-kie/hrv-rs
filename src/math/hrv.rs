//! HRV (Heart Rate Variability) Computation
//!
//! This module contains functions and utilities for calculating HRV metrics.
//! It provides statistical and frequency-based methods for HRV analysis.

use log::trace;
use nalgebra::{DMatrix, DVector};

/// `calc_rmssd` function.
///
/// Calculates RMSSD (Root Mean Square of Successive Differences).
///
/// # Arguments
/// - `data`: A slice of RR intervals in milliseconds.
///
/// # Returns
/// RMSSD value as a `f64`.
///
/// # Panics
/// Panics if the input slice has less than 2 elements.
pub fn calc_rmssd(data: &[f64]) -> f64 {
    assert!(
        data.len() > 1,
        "Data must contain at least two elements for RMSSD calculation."
    );

    let rr_points_a = DVector::from_row_slice(&data[0..data.len() - 1]);
    let rr_points_b = DVector::from_row_slice(&data[1..]);
    let successive_diffs = rr_points_b - rr_points_a;

    trace!(
        "Calculating RMSSD with successive differences: {:?}",
        successive_diffs
    );
    (successive_diffs.dot(&successive_diffs) / (successive_diffs.len() as f64)).sqrt()
}

/// `calc_sdrr` function.
///
/// Calculates SDRR (Standard Deviation of RR intervals).
///
/// # Arguments
/// - `data`: A slice of RR intervals in milliseconds.
///
/// # Returns
/// SDRR value as a `f64`.
///
/// # Panics
/// Panics if the input slice has less than 2 elements.
pub fn calc_sdrr(data: &[f64]) -> f64 {
    assert!(
        data.len() > 1,
        "Data must contain at least two elements for SDRR calculation."
    );

    let variance = DVector::from_row_slice(data).variance();
    trace!("Calculating SDRR with variance: {}", variance);
    variance.sqrt()
}

/// Results of Poincare plot metrics.
#[derive(Clone, Copy, Default)]
/// `PoincareAnalysisResult` structure.
///
/// Stores results of Poincare plot analysis, including SD1, SD2, and their eigenvectors.
pub struct PoincareAnalysisResult {
    pub sd1: f64,
    pub sd1_eigenvector: [f64; 2],
    pub sd2: f64,
    pub sd2_eigenvector: [f64; 2],
}

/// `calc_poincare_metrics` function.
///
/// Calculates Poincare plot metrics SD1 and SD2 with their eigenvectors.
///
/// # Arguments
/// - `data`: A slice of RR intervals in milliseconds.
///
/// # Returns
/// A `PoincareAnalysisResult` containing SD1, SD2, and their eigenvectors.
///
/// # Panics
/// Panics if the input slice has less than 2 elements.
pub fn calc_poincare_metrics(data: &[f64]) -> PoincareAnalysisResult {
    assert!(
        data.len() > 1,
        "Data must contain at least two elements for Poincare metrics calculation."
    );

    let rr_points_a = DVector::from_row_slice(&data[0..data.len() - 1]);
    let rr_points_b = DVector::from_row_slice(&data[1..]);

    // Center the data
    let poincare_matrix = {
        let mut centered = DMatrix::from_columns(&[rr_points_a, rr_points_b]);
        let col_means = centered.row_mean();
        for mut row in centered.row_iter_mut() {
            row -= &col_means;
        }
        centered
    };

    trace!("Poincare matrix:\n{:?}", poincare_matrix);

    // Covariance matrix and eigen decomposition
    let poincare_cov =
        poincare_matrix.transpose() * &poincare_matrix / (poincare_matrix.nrows() as f64 - 1.0);
    let ev = nalgebra::SymmetricEigen::new(poincare_cov);

    PoincareAnalysisResult {
        sd1: ev.eigenvalues[0].sqrt(),
        sd1_eigenvector: [ev.eigenvectors.column(0)[0], ev.eigenvectors.column(0)[1]],
        sd2: ev.eigenvalues[1].sqrt(),
        sd2_eigenvector: [ev.eigenvectors.column(1)[0], ev.eigenvectors.column(1)[1]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rmssd() {
        let data = [1000.0, 1010.0, 1020.0, 1030.0, 1040.0];
        let rmssd = calc_rmssd(&data);
        assert!(rmssd > 0.0, "RMSSD should be positive.");
    }

    #[test]
    fn test_sdrr() {
        let data = [1000.0, 1010.0, 1020.0, 1030.0, 1040.0];
        let sdrr = calc_sdrr(&data);
        assert!(sdrr > 0.0, "SDRR should be positive.");
    }

    #[test]
    fn test_poincare_metrics() {
        let data = [1000.0, 1010.0, 1001.0, 1030.0, 1049.0];
        let poincare = calc_poincare_metrics(&data);
        assert!(poincare.sd1 > 0.0, "SD1 should be positive.");
        assert!(poincare.sd2 > 0.0, "SD2 should be positive.");
        assert!(
            poincare.sd1_eigenvector[0] != 0.0,
            "SD1 eigenvector should not be zero."
        );
        assert!(
            poincare.sd2_eigenvector[0] != 0.0,
            "SD2 eigenvector should not be zero."
        );
    }
}
