# Observability Documentation

This document describes the observability infrastructure for the H-5N1P3R system, including distributed tracing, metrics collection, and monitoring setup.

## Overview

The H-5N1P3R system uses OpenTelemetry for comprehensive observability:

- **Distributed Tracing**: Track requests across async boundaries and actors using OpenTelemetry traces exported to Jaeger
- **Metrics**: System and application metrics exported to Prometheus
- **Logging**: Structured logging with tracing integration

## Architecture

```
┌─────────────────┐
│   Application   │
│   (H-5N1P3R)    │
└────────┬────────┘
         │
         ├─── Traces (OTLP/gRPC) ──→ Jaeger
         │
         └─── Metrics (Pull) ──────→ Prometheus ──→ Grafana
```

## Key Components

### 1. Tracing

Distributed tracing is implemented using OpenTelemetry and exported to Jaeger via OTLP (OpenTelemetry Protocol).

**Instrumented Components:**
- `PredictiveOracle::score_candidate()` - Oracle scoring operations
- `PredictiveOracle::record_trade_outcome()` - Trade outcome recording
- `DecisionLedger::run()` - Storage operations
- `TransactionMonitor::run()` - Transaction monitoring
- `OracleActor` handlers - Actor message processing

**Span Attributes:**
- `mint`: Token mint address being scored
- `service.name`: "h-5n1p3r"
- `service.version`: Package version
- `deployment.environment`: Environment (dev/staging/prod)

### 2. Metrics

Prometheus-compatible metrics are available via the metrics endpoint.

**Available Metrics:**
- Oracle scoring time (histograms)
- Transaction processing rates (counters)
- Cache hit/miss rates (counters)
- Actor message queue depths (gauges)
- Pattern memory statistics (gauges)
- Metacognition calibration metrics (gauges)

### 3. Context Propagation

Trace context is automatically propagated across:
- Tokio async tasks
- Actor message passing (via Actix)
- RPC calls (via instrumented HTTP clients)
- Channel communications

## Setup

### Local Development

#### 1. Start Jaeger (All-in-One)

```bash
# Using Docker
docker run -d --name jaeger \
  -e COLLECTOR_OTLP_ENABLED=true \
  -p 16686:16686 \
  -p 4317:4317 \
  -p 4318:4318 \
  jaegertracing/all-in-one:latest

# Or using Docker Compose (see docker-compose.yml below)
docker-compose up -d jaeger
```

**Jaeger UI**: http://localhost:16686

#### 2. Start Prometheus

Create `prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'h-5n1p3r'
    static_configs:
      - targets: ['host.docker.internal:9090']
    # Or for Linux: 
    # - targets: ['172.17.0.1:9090']
```

```bash
# Using Docker
docker run -d --name prometheus \
  -p 9090:9090 \
  -v $(pwd)/prometheus.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus

# Or using Docker Compose
docker-compose up -d prometheus
```

**Prometheus UI**: http://localhost:9090

#### 3. Start Grafana (Optional)

```bash
# Using Docker
docker run -d --name grafana \
  -p 3000:3000 \
  grafana/grafana

# Or using Docker Compose
docker-compose up -d grafana
```

**Grafana UI**: http://localhost:3000 (default credentials: admin/admin)

#### 4. Configure Application

Set environment variables:

```bash
# Enable tracing (disabled by default in local dev)
export ENABLE_TRACING=true

# Enable metrics (enabled by default)
export ENABLE_METRICS=true

# OTLP endpoint for Jaeger (default: http://localhost:4317)
export OTLP_ENDPOINT=http://localhost:4317

# Environment tag
export ENVIRONMENT=dev

# Trace sampling ratio (0.0 to 1.0, default: 1.0 = sample all)
export TRACE_SAMPLING_RATIO=1.0

# Log level
export RUST_LOG=info,h_5n1p3r=debug
```

#### 5. Run Application

```bash
cd bot
cargo run
```

### Docker Compose Setup

Create `docker-compose.yml` in the bot directory:

```yaml
version: '3.8'

services:
  jaeger:
    image: jaegertracing/all-in-one:latest
    container_name: jaeger
    environment:
      - COLLECTOR_OTLP_ENABLED=true
    ports:
      - "16686:16686"  # Jaeger UI
      - "4317:4317"    # OTLP gRPC
      - "4318:4318"    # OTLP HTTP
    networks:
      - observability

  prometheus:
    image: prom/prometheus:latest
    container_name: prometheus
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus-data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
    networks:
      - observability

  grafana:
    image: grafana/grafana:latest
    container_name: grafana
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
      - GF_USERS_ALLOW_SIGN_UP=false
    volumes:
      - grafana-data:/var/lib/grafana
    depends_on:
      - prometheus
    networks:
      - observability

networks:
  observability:
    driver: bridge

volumes:
  prometheus-data:
  grafana-data:
```

Start everything:

```bash
docker-compose up -d
```

### Production Deployment

#### 1. Jaeger Setup

For production, use a proper Jaeger deployment with persistent storage:

**Option A: Jaeger with Elasticsearch**

```bash
# Deploy Elasticsearch
# Deploy Jaeger Collector, Query, and Agent with Elasticsearch backend

# Set application environment
export OTLP_ENDPOINT=http://jaeger-collector:4317
export ENVIRONMENT=production
export TRACE_SAMPLING_RATIO=0.1  # Sample 10% of traces
```

**Option B: Jaeger with Kafka**

For high-throughput environments, use Kafka as a buffer between collectors and storage.

#### 2. Prometheus Setup

```yaml
# prometheus.yml for production
global:
  scrape_interval: 15s
  evaluation_interval: 15s
  external_labels:
    cluster: 'production'
    environment: 'prod'

scrape_configs:
  - job_name: 'h-5n1p3r'
    static_configs:
      - targets: 
          - 'h-5n1p3r-1:9090'
          - 'h-5n1p3r-2:9090'
          - 'h-5n1p3r-3:9090'
    relabel_configs:
      - source_labels: [__address__]
        target_label: instance

# Add alerting rules
rule_files:
  - 'alerts.yml'

alerting:
  alertmanagers:
    - static_configs:
        - targets: ['alertmanager:9093']
```

#### 3. Grafana Dashboards

Import pre-built dashboards:

1. **Jaeger Data Source**:
   - Configuration → Data Sources → Add data source
   - Select "Jaeger"
   - URL: `http://jaeger:16686`

2. **Prometheus Data Source**:
   - Configuration → Data Sources → Add data source
   - Select "Prometheus"
   - URL: `http://prometheus:9090`

3. **Import Dashboards**:
   - Use dashboard ID: 12230 (Jaeger Dashboard)
   - Use dashboard ID: 1860 (Node Exporter)
   - Create custom dashboards for H-5N1P3R metrics

#### 4. Application Configuration

```bash
# Production environment variables
export ENABLE_TRACING=true
export ENABLE_METRICS=true
export OTLP_ENDPOINT=http://jaeger-collector:4317
export ENVIRONMENT=production
export TRACE_SAMPLING_RATIO=0.1
export RUST_LOG=info,h_5n1p3r=info
```

## Monitoring Queries

### Prometheus Queries

```promql
# Average scoring time
rate(oracle_scoring_duration_seconds_sum[5m]) / rate(oracle_scoring_duration_seconds_count[5m])

# Scoring operations per second
rate(oracle_scoring_duration_seconds_count[5m])

# Cache hit rate
rate(oracle_cache_hits_total[5m]) / (rate(oracle_cache_hits_total[5m]) + rate(oracle_cache_misses_total[5m]))

# Transaction processing rate
rate(transaction_processed_total[5m])

# Pattern memory success rate
pattern_memory_success_rate
```

### Jaeger Queries

Search for traces in the Jaeger UI:

- **Service**: `h-5n1p3r`
- **Operation**: 
  - `score_candidate` - Oracle scoring operations
  - `record_trade_outcome` - Trade outcome recording
  - `decision_ledger_run` - Storage operations
  - `transaction_monitor_run` - Transaction monitoring

Filter by:
- **Tags**: `mint`, `environment`
- **Duration**: Find slow operations
- **Errors**: Identify failures

## Debugging

### View Active Traces

```bash
# Check if traces are being exported
docker logs jaeger 2>&1 | grep -i "spans received"

# Check Jaeger UI for traces
open http://localhost:16686
```

### View Metrics

```bash
# Check Prometheus targets
open http://localhost:9090/targets

# Query metrics directly
curl http://localhost:9090/api/v1/query?query=up
```

### Application Logs

```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run with trace-level OpenTelemetry logs
RUST_LOG=opentelemetry=trace,h_5n1p3r=debug cargo run
```

## Troubleshooting

### Traces Not Appearing in Jaeger

1. **Check OTLP endpoint**:
   ```bash
   curl -v http://localhost:4317
   ```

2. **Verify application logs**:
   ```bash
   grep -i "observability initialized" app.log
   grep -i "opentelemetry" app.log
   ```

3. **Check Jaeger collector**:
   ```bash
   docker logs jaeger 2>&1 | grep -i error
   ```

4. **Verify ENABLE_TRACING is set**:
   ```bash
   echo $ENABLE_TRACING
   ```

### Metrics Not Appearing in Prometheus

1. **Check metrics endpoint**:
   ```bash
   curl http://localhost:9090/metrics
   ```

2. **Verify Prometheus config**:
   ```bash
   docker exec prometheus cat /etc/prometheus/prometheus.yml
   ```

3. **Check Prometheus targets**:
   Open http://localhost:9090/targets and verify the target is UP

### Performance Impact

Observability has minimal performance impact:

- **Tracing**: ~1-5% CPU overhead with 100% sampling
- **Metrics**: ~0.5% CPU overhead
- **Memory**: ~50MB additional for trace buffers

For production:
- Reduce sampling ratio to 0.1 (10%)
- Use proper Jaeger storage backend
- Configure retention policies

## Custom Instrumentation

### Adding Spans to Functions

```rust
use tracing::instrument;

#[instrument(skip(some_arg), fields(important_field = %some_arg.field))]
async fn my_function(some_arg: MyType) -> Result<()> {
    // Function body
    // Span is automatically created and closed
    Ok(())
}
```

### Adding Custom Attributes

```rust
use tracing::{info_span, Instrument};

async fn process_data(data: &Data) {
    let span = info_span!("process_data", data_id = %data.id, size = data.len());
    
    async {
        // Processing logic
    }.instrument(span).await;
}
```

### Recording Events

```rust
use tracing::{info, warn, error};

info!(target: "oracle", "Scoring candidate {}", mint);
warn!(latency_ms = 150, "High latency detected");
error!(error = %e, "Failed to process request");
```

## Dashboards

### Grafana Dashboard JSON

Import the following dashboard template for H-5N1P3R metrics:

```json
{
  "dashboard": {
    "title": "H-5N1P3R System Overview",
    "panels": [
      {
        "title": "Oracle Scoring Rate",
        "targets": [
          {
            "expr": "rate(oracle_scoring_duration_seconds_count[5m])"
          }
        ]
      },
      {
        "title": "Average Scoring Time",
        "targets": [
          {
            "expr": "rate(oracle_scoring_duration_seconds_sum[5m]) / rate(oracle_scoring_duration_seconds_count[5m])"
          }
        ]
      },
      {
        "title": "Cache Hit Rate",
        "targets": [
          {
            "expr": "rate(oracle_cache_hits_total[5m]) / (rate(oracle_cache_hits_total[5m]) + rate(oracle_cache_misses_total[5m]))"
          }
        ]
      }
    ]
  }
}
```

## Best Practices

1. **Sampling**: In production, use sampling ratio of 0.05-0.1 (5-10%)
2. **Span Naming**: Use clear, hierarchical span names (e.g., `oracle.score_candidate`)
3. **Attributes**: Add business-relevant attributes (e.g., `mint`, `score`, `confidence`)
4. **Error Handling**: Always record errors in spans with `error = true`
5. **Context Propagation**: Ensure trace context flows through all async boundaries
6. **Retention**: Set appropriate retention policies (7-30 days for traces, 90+ days for metrics)
7. **Alerts**: Configure alerts for critical metrics (error rates, latencies, throughput)

## Security Considerations

1. **PII in Traces**: Avoid logging sensitive information in span attributes
2. **Metrics Endpoint**: Secure the Prometheus metrics endpoint with authentication
3. **Jaeger Access**: Restrict access to Jaeger UI in production
4. **Network Security**: Use TLS for OTLP exports in production

## References

- [OpenTelemetry Documentation](https://opentelemetry.io/docs/)
- [Jaeger Documentation](https://www.jaegertracing.io/docs/)
- [Prometheus Documentation](https://prometheus.io/docs/)
- [Grafana Documentation](https://grafana.com/docs/)
- [Tracing Crate](https://docs.rs/tracing/)
- [OpenTelemetry Rust SDK](https://docs.rs/opentelemetry/)
