use crate::indicators;
use crate::models::PriceData;
use crate::traits::PriceRepository;
use serde::{Deserialize, Serialize};

const FEATURE_COUNT: usize = 11;
const MIN_TRAIN_SAMPLES: usize = 10;
const EPOCHS: usize = 2000;
const L2_REG: f64 = 0.01;
const LR_INIT: f64 = 0.1;
const LR_DECAY: f64 = 0.001;
const TRAIN_SPLIT: f64 = 0.8;

#[derive(Debug, Serialize, Deserialize)]
pub struct Model {
    weights: Vec<f64>,
    bias: f64,
    feature_mean: Vec<f64>,
    feature_std: Vec<f64>,
    accuracy: f64,
}

impl Model {
    fn sigmoid(z: f64) -> f64 {
        1.0 / (1.0 + (-z).exp())
    }

    fn predict_proba(&self, features: &[f64]) -> f64 {
        let mut z = self.bias;
        for (i, &f) in features.iter().enumerate() {
            let normalized = (f - self.feature_mean[i]) / self.feature_std[i].max(1e-10);
            z += self.weights[i] * normalized;
        }
        Self::sigmoid(z)
    }

    pub fn predict(&self, features: &[f64]) -> bool {
        self.predict_proba(features) > 0.5
    }

    pub fn confidence(&self, features: &[f64]) -> f64 {
        let p = self.predict_proba(features);
        if p > 0.5 { p } else { 1.0 - p }
    }

    #[allow(dead_code)]
    pub fn accuracy(&self) -> f64 {
        self.accuracy
    }

    #[allow(dead_code)]
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
}

pub struct Dataset {
    pub features: Vec<Vec<f64>>,
    pub labels: Vec<f64>,
    #[allow(dead_code)]
    pub timestamps: Vec<i64>,
}

pub fn build_dataset(data: &PriceData) -> Dataset {
    let (macd_series, signal_series) = indicators::compute_macd_series(&data.closes);
    let rsi_series = indicators::compute_rsi_series(&data.closes, 14);
    let adx_series = indicators::compute_adx_series(&data.highs, &data.lows, &data.closes, 14);
    let atr_series = indicators::compute_atr_series(&data.highs, &data.lows, &data.closes, 14);
    let ret1 = indicators::compute_return_series(&data.closes, 1);
    let ret5 = indicators::compute_return_series(&data.closes, 5);
    let ret21 = indicators::compute_return_series(&data.closes, 21);
    let vol_21 = indicators::compute_rolling_volatility(&data.closes, 21);
    let obv = indicators::compute_obv(&data.closes, &data.volume);

    let mut features = Vec::new();
    let mut labels = Vec::new();
    let mut timestamps = Vec::new();

    for i in 35..data.closes.len().saturating_sub(1) {
        match (macd_series[i], signal_series[i], rsi_series[i], adx_series[i]) {
            (Some(m), Some(s), Some(r), Some(a)) => {
                let atr = atr_series[i].unwrap_or(0.0);
                let atr_norm = atr / data.closes[i].max(1e-10);
                let lr1 = ret1[i].unwrap_or(0.0);
                let lr5 = ret5[i].unwrap_or(0.0);
                let lr21 = ret21[i].unwrap_or(0.0);
                let v21 = vol_21[i].unwrap_or(0.0);
                let obv_val = obv[i].unwrap_or(0.0);

                features.push(vec![m, s, m - s, r, a, atr_norm, lr1, lr5, lr21, v21, obv_val]);
                labels.push(if data.closes[i + 1] > data.closes[i] { 1.0 } else { 0.0 });
                timestamps.push(data.timestamps[i]);
            }
            _ => continue,
        }
    }

    Dataset { features, labels, timestamps }
}

pub fn train(dataset: &Dataset) -> Model {
    let n = dataset.features.len();
    let n_features = if n > 0 { dataset.features[0].len() } else { 0 };

    if n < MIN_TRAIN_SAMPLES || n_features == 0 {
        return Model {
            weights: vec![0.0; n_features.max(1)],
            bias: 0.0,
            feature_mean: vec![0.0; n_features.max(1)],
            feature_std: vec![1.0; n_features.max(1)],
            accuracy: 0.0,
        };
    }

    // Z-score normalization
    let mut mean = vec![0.0; n_features];
    let mut std = vec![0.0; n_features];
    for feat in &dataset.features {
        for (j, &v) in feat.iter().enumerate() {
            mean[j] += v;
        }
    }
    for m in mean.iter_mut() {
        *m /= n as f64;
    }
    for feat in &dataset.features {
        for (j, &v) in feat.iter().enumerate() {
            std[j] += (v - mean[j]).powi(2);
        }
    }
    for s in std.iter_mut() {
        *s = (*s / n as f64).sqrt().max(1e-10);
    }

    // Train/test split (80/20, preserving temporal order)
    let split = (n as f64 * TRAIN_SPLIT) as usize;
    let (train_x, test_x) = (&dataset.features[..split], &dataset.features[split..]);
    let (train_y, test_y) = (&dataset.labels[..split], &dataset.labels[split..]);
    let train_n = train_x.len();

    // Normalize train features
    let train_norm: Vec<Vec<f64>> = train_x
        .iter()
        .map(|f| {
            f.iter()
                .enumerate()
                .map(|(j, &v)| (v - mean[j]) / std[j])
                .collect()
        })
        .collect();

    // Initialize weights
    let mut w = vec![0.0; n_features];
    let mut b = 0.0;

    // SGD training
    for epoch in 0..EPOCHS {
        let lr = LR_INIT / (1.0 + LR_DECAY * epoch as f64);
        let mut total_loss = 0.0;

        // Shuffle
        let mut indices: Vec<usize> = (0..train_n).collect();
        for i in (1..train_n).rev() {
            let j = (epoch * 7 + i * 13) % (i + 1);
            indices.swap(i, j);
        }

        for &idx in &indices {
            let mut z = b;
            for (j, &v) in train_norm[idx].iter().enumerate() {
                z += w[j] * v;
            }
            let pred = Model::sigmoid(z);
            let err = pred - train_y[idx];

            for j in 0..n_features {
                w[j] -= lr * (err * train_norm[idx][j] + L2_REG * w[j]);
            }
            b -= lr * err;

            let eps = 1e-10;
            total_loss += if train_y[idx] > 0.5 {
                -(train_y[idx] * pred.ln().max(eps))
            } else {
                -((1.0 - train_y[idx]) * (1.0 - pred + eps).ln())
            };
        }

        if epoch % 500 == 0 {
            let _loss = total_loss / train_n as f64;
        }
    }

    // Evaluate on test set
    let mut correct = 0;
    for i in 0..test_x.len() {
        let mut z = b;
        for (j, &v) in test_x[i].iter().enumerate() {
            z += w[j] * (v - mean[j]) / std[j];
        }
        let pred = Model::sigmoid(z);
        let predicted = if pred > 0.5 { 1.0 } else { 0.0 };
        if (predicted - test_y[i]).abs() < 0.5 {
            correct += 1;
        }
    }
    let accuracy = correct as f64 / test_x.len().max(1) as f64;

    Model {
        weights: w,
        bias: b,
        feature_mean: mean,
        feature_std: std,
        accuracy,
    }
}

fn latest_features(data: &PriceData) -> Option<Vec<f64>> {
    let (macd_line, signal) = indicators::compute_macd(&data.closes);
    let rsi_val = indicators::compute_rsi(&data.closes, 14);
    let adx_val = indicators::compute_adx(&data.highs, &data.lows, &data.closes, 14);
    let atr_val = indicators::compute_atr(&data.highs, &data.lows, &data.closes, 14);
    let ret1 = indicators::compute_return_series(&data.closes, 1);
    let ret5 = indicators::compute_return_series(&data.closes, 5);
    let ret21 = indicators::compute_return_series(&data.closes, 21);
    let vol_21 = indicators::compute_rolling_volatility(&data.closes, 21);
    let obv = indicators::compute_obv(&data.closes, &data.volume);

    let last = data.closes.len() - 1;
    let atr_norm = atr_val.unwrap_or(0.0) / data.closes[last].max(1e-10);
    let lr1 = ret1[last].unwrap_or(0.0);
    let lr5 = ret5[last].unwrap_or(0.0);
    let lr21 = ret21[last].unwrap_or(0.0);
    let v21 = vol_21[last].unwrap_or(0.0);
    let obv_val = obv[last].unwrap_or(0.0);

    match (macd_line, signal, rsi_val, adx_val) {
        (Some(m), Some(s), Some(r), Some(a)) => {
            Some(vec![m, s, m - s, r, a, atr_norm, lr1, lr5, lr21, v21, obv_val])
        }
        _ => None,
    }
}

pub fn predict_next(data: &PriceData, symbol: &str, repo: Option<&dyn PriceRepository>) -> Option<(bool, f64, f64)> {
    if let Some(repo) = repo {
        if let Ok(Some(json)) = repo.load_model_json(symbol, data.closes.len()) {
            if let Ok(model) = serde_json::from_str::<Model>(&json) {
                if model.weights.len() == FEATURE_COUNT {
                    let feats = latest_features(data)?;
                    return Some((model.predict(&feats), model.confidence(&feats), model.accuracy));
                }
            }
        }
    }

    let dataset = build_dataset(data);
    if dataset.features.len() < MIN_TRAIN_SAMPLES {
        return None;
    }
    let model = train(&dataset);

    if let Some(repo) = repo {
        if let Ok(json) = serde_json::to_string(&model) {
            let _ = repo.save_model_json(symbol, &json, data.closes.len());
        }
    }

    let feats = latest_features(data)?;
    Some((model.predict(&feats), model.confidence(&feats), model.accuracy))
}
