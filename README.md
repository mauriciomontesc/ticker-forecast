# ticker-forecast — Análisis técnico y ML para forex/equities

CLI tool que combina indicadores técnicos clásicos con regresión logística para generar señales de trading direccional.

## Arquitectura

```
api.rs        → Fetch OHLCV desde Yahoo Finance
db.rs         → Caché SQLite + persistencia del modelo ML
indicators.rs → Cálculo de indicadores técnicos (MACD, RSI, ADX, ATR, OBV, etc.)
forecast.rs   → Sistema de votación ponderada + decisión final
ml.rs         → Regresión logística con SGD + z-score normalization
display.rs    → Output por consola via tracing
traits.rs     → Abstracción PriceRepository
```

## Features del modelo ML (11 dimensiones)

Cada sample de entrenamiento es un vector de 11 features extraídas de una ventana de precios histórica. La variable objetivo es `1` si el precio de cierre del día siguiente sube, `0` si baja.

| # | Feature | Descripción | Fuente |
|---|---------|-------------|--------|
| 0 | `macd_line` | EMA12 - EMA26 | MACD |
| 1 | `signal_line` | EMA9 de la línea MACD | MACD |
| 2 | `histogram` | macd_line - signal_line | MACD |
| 3 | `rsi` | Relative Strength Index (14) | RSI |
| 4 | `adx` | Average Directional Index (14) | ADX |
| 5 | `atr_norm` | ATR(14) / close_price | ATR |
| 6 | `log_return_1` | ln(close[i] / close[i-1]) | Retornos |
| 7 | `log_return_5` | ln(close[i] / close[i-5]) | Retornos |
| 8 | `log_return_21` | ln(close[i] / close[i-21]) | Retornos |
| 9 | `volatility_21` | Desv. estándar de retornos a 21d | Volatilidad |
| 10 | `obv` | On-Balance Volume acumulado | Volumen |

### Pre-procesamiento

- **Z-score normalization**: cada feature se normaliza antes de entrenar: `(x - μ) / σ`
- **Train/test split**: 80/20 preservando orden temporal (no shuffle aleatorio, se usa el 80% más antiguo para entrenar y el 20% más reciente para evaluar)
- **Mínimo de samples**: 10 (con menos datos no se entrena)

## Modelo: Regresión Logística

```
P(y=1 | x) = sigmoid(w·x + b)

sigmoid(z) = 1 / (1 + e^(-z))
```

### Entrenamiento

- **Algoritmo**: SGD (Stochastic Gradient Descent) con mini-batch de 1
- **Epochs**: 2000
- **Learning rate**: `α = 0.1 / (1 + 0.001 * epoch)` (decaimiento inverso)
- **Regularización**: L2 con `λ = 0.01`
- **Inicialización**: todos los pesos en 0
- **Loss**: Binary Cross-Entropy (log loss)
- **Shuffle**: pseudo-aleatorio determinista por epoch (no shuffle real, para reproducibilidad)

### Persistencia

El modelo entrenado se guarda en SQLite (`ml_model` table) como JSON serializado, keyeado por símbolo y cantidad de datos. En la siguiente invocación con el mismo símbolo y misma cantidad de datos se reusa el modelo cacheado, evitando re-entrenar.

Si el feature vector cambia (por ej. al agregar nuevas features), se detecta por mismatch en `weights.len()` y se re-entrena automáticamente.

## Sistema de decisión (forecast)

El forecast combina dos fuentes:

### 1. Indicadores técnicos (votación ponderada)

| Señal | Peso | Dirección |
|-------|------|-----------|
| MACD histogram | `\|histogram\| / (1 + \|histogram\|)` | `macd > signal` → bullish |
| RSI | `\|rsi - 50\| / 50` | `rsi > 50` → bullish |

ADX no vota, solo modula la confianza del mensaje:
- ADX > 25 → tendencia presente (señales más confiables)
- ADX < 25 → mercado lateral (señales débiles)

### 2. ML (voto ponderado por accuracy)

| Accuracy | Peso del voto |
|----------|--------------|
| > 0.70 | ±3.0 |
| 0.60 – 0.70 | ±2.0 |
| 0.50 – 0.60 | ±1.0 |
| < 0.50 | ignorado |

### Decisión final

```
bullish > bearish → Bullish
bearish > bullish → Bearish
else → Neutral
```

## Sesgo conocido

El modelo predice dirección 1-periodo adelante con features del periodo actual. Esto no es forward-looking estricto — se asume que todas las features están disponibles al cierre del día t para predecir el movimiento t+1. No hay leak porque no se usan datos de t+1 en las features. Sin embargo, en backtesting real se debe considerar el spread, slippage y comisiones.

## Uso

```
ticker-forecast [SYMBOL] [--refresh] [--intraday] [--range RANGE] [--interval INTERVAL]

Ejemplos:
  ticker-forecast                  # EURUSD=X, desde caché
  ticker-forecast AAPL --refresh   # Fuerza descarga de AAPL
  ticker-forecast --intraday       # EURUSD=X en timeframe 1h
```

## Tests

```
cargo test
```

21 tests unitarios cubren: MACD, RSI, ADX, ATR, retornos, volatilidad, OBV, series de indicadores, forecast ponderado, edge cases con datos insuficientes.
