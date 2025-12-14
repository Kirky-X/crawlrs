# Operations Documentation

## Overview
Crawlrs is a high-performance, distributed web crawler built with Rust. This document outlines the procedures for deployment, monitoring, and maintenance.

## Architecture
- **API Service**: Handles RESTful requests, manages crawl jobs, and serves metrics.
- **Worker Service**: Consumes tasks from the queue and performs the actual crawling/scraping.
- **PostgreSQL**: Persistent storage for crawl jobs, tasks, and results.
- **Redis**: High-speed task queue and caching.

## Deployment

### Prerequisites
- Kubernetes Cluster (v1.20+)
- `kubectl` configured
- Docker Registry access

### Kubernetes Deployment
1. **Secrets Management**:
   Create a `k8s/secrets.yaml` file (template provided) with your actual credentials:
   ```yaml
   apiVersion: v1
   kind: Secret
   metadata:
     name: crawlrs-secrets
   type: Opaque
   data:
     DATABASE_URL: <base64-encoded-url>
     REDIS_URL: <base64-encoded-url>
     LLM_API_KEY: <base64-encoded-key>
   ```
   Apply it:
   ```bash
   kubectl apply -f k8s/secrets.yaml
   ```

2. **Deploy Services**:
   Deploy the API and Worker components:
   ```bash
   kubectl apply -f k8s/api-deployment.yaml
   kubectl apply -f k8s/worker-deployment.yaml
   ```

3. **Verification**:
   Check the status of pods:
   ```bash
   kubectl get pods
   ```

## Monitoring & Observability

### Metrics
The application exposes Prometheus-compatible metrics at port `9000` on the `/metrics` endpoint.
- **API Metrics**: Request latency, status codes, active connections.
- **Worker Metrics**: Tasks processed, success/failure rates, queue depth.

### Grafana Dashboards
A pre-configured dashboard is available at `grafana/dashboards/overview.json`.
1. Log in to Grafana.
2. Go to **Dashboards > Import**.
3. Upload the JSON file or paste its content.

## Testing & Validation

### End-to-End (E2E) Tests
Run the Python-based E2E test suite to verify core functionality:
```bash
# Install dependencies
pip install requests

# Run tests
python3 tests/e2e/test_scenarios.py
```

### Stress Testing
Use k6 to simulate high load:
```bash
# Install k6
sudo gpg -k
sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys C5AD17C747E3415A3642D57D77C6C49186013262
echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update
sudo apt-get install k6

# Run stress test
k6 run tests/stress/k6_script.js
```

### Performance Benchmarking
Run Rust micro-benchmarks:
```bash
cargo bench
```

## Troubleshooting

### Common Issues
- **Database Connection Failed**: Check `DATABASE_URL` in secrets and ensure Postgres is running.
- **Worker Stalls**: Check Redis connectivity and queue depth.
- **LLM Extraction Failures**: Verify `LLM_API_KEY` and quota limits.

### Logs
Access logs via kubectl:
```bash
kubectl logs -l app=crawlrs-api
kubectl logs -l app=crawlrs-worker
```
