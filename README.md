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

## Features del modelo ML (13 dimensiones)

Cada sample de entrenamiento es un vector de 13 features extraídas de una ventana de precios histórica. La variable objetivo es `1` si el precio de cierre del día siguiente sube, `0` si baja.

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
| 11 | `bollinger_b` | Bollinger %B (20, 2σ) | Bollinger |
| 12 | `stochastic_k` | Stochastic %K (14) | Stochastic |

### Pre-procesamiento

- **Z-score normalization**: cada feature se normaliza con media y desviación calculadas **solo sobre el split de entrenamiento** (80% más antiguo), evitando leakage del test set
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

## Caché de datos OHLCV

Los datos descargados se guardan en SQLite (`trade_cache.db`) con clave compuesta:

```
(symbol, range, interval)
```

- **TTL**: 3 días, basado en `fetched_at` (momento de la descarga), no en la fecha del último candle
- **Aislamiento**: datos daily (`1d`) e intraday (`1h`) no se mezclan aunque sea el mismo símbolo
- **`--refresh`**: fuerza descarga y reemplaza la serie completa para esa clave

## Sistema de decisión (forecast)

El forecast combina dos fuentes:

### 1. Indicadores técnicos (votación ponderada)

| Señal | Peso | Dirección |
|-------|------|-----------|
| MACD histogram | `\|histogram\| / (1 + \|histogram\|)` | `macd > signal` → bullish |
| RSI | `\|rsi - 50\| / 50` | `rsi > 50` → bullish |
| Bollinger %B | `\|%B - 0.5\| × 0.4` | `%B < 0.5` → bullish |
| Stochastic %K | `\|K - 50\| / 50 × 0.3` | `K < 50` → bullish |
| Soporte cercano | `+0.4` si dist < 2%, `+0.2` si dist < 5% | precio cerca de soporte → bullish |
| Resistencia cercana | `+0.4` si dist < 2%, `+0.2` si dist < 5% | precio cerca de resistencia → bearish |

ADX no vota, solo modula la confianza del mensaje:
- ADX > 25 → tendencia presente (señales más confiables)
- ADX < 25 → mercado lateral (señales débiles)

Bollinger bandwidth y niveles S/R se muestran en el output pero solo %B y proximidad S/R participan en la votación.

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

## Pronóstico a largo plazo

El CLI genera **dos pronósticos** en cada ejecución:

1. **Corto plazo** (próxima vela): voto ponderado de indicadores técnicos + regresión logística (ML).
2. **Largo plazo** (~1 mes): regresión lineal sobre el **logaritmo** de los closes recientes. El retorno proyectado es `exp(slope × horizonte) − 1`, donde `slope` es el retorno logarítmico medio por vela; se muestra como % y como precio proyectado.

- **Horizonte**: 21 sesiones de trading (~1 mes). Para datos intradía se deriva del ritmo real de las velas (p. ej. forex 24h ≈ 504 velas de 1h; equities 6.5h ≈ 134), con una tabla por intervalo como fallback.
- **Ventana**: la tendencia se ajusta solo sobre las últimas `3 × horizonte` velas (≈3 meses), no sobre todo el histórico, para no diluir cambios de régimen recientes.
- **Dirección**: alcista si el retorno proyectado supera ±0.5%; bajista si cae por debajo; en caso contrario `Neutral`.
- **Datos mínimos**: `max(35, 3 × horizonte)` velas; si no alcanzan, el pronóstico de largo plazo se omite con un warning.
- **Independiente** del voto del ML de corto plazo; ambos comparten el mismo snapshot de indicadores (se calculan una sola vez).

## Tests

```
cargo test
```

57 tests unitarios cubren: MACD, RSI, ADX, ATR, retornos, volatilidad, OBV, Bollinger, Stochastic, series de indicadores, forecast ponderado, forecast de largo plazo (log-retornos, ventana reciente, horizonte por intervalo/cadencia), caché SQLite, normalización ML y edge cases con datos insuficientes.
