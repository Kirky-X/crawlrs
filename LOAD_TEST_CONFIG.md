# crawlrs Load Test Configuration

## Overview

This document describes the load testing configuration for crawlrs API performance validation.

## K6 Load Test Script

**Location**: `tests/stress/k6_script.js`

### Test Scenarios

#### 1. Baseline Load Test
```bash
k6 run tests/stress/k6_script.js
```

**Configuration**:
- **Ramp Up**: 30s → 50 users
- **Load**: 1m → 200 users
- **Peak**: 1m → 500 users, 2m → 1000 users
- **Sustain**: 2m → 1000 users
- **Ramp Down**: 30s → 0 users

**Total Duration**: ~7 minutes

#### 2. High Concurrency Test
```javascript
// Modified options for high concurrency
export let options = {
    stages: [
        { duration: '1m', target: 1000 },
        { duration: '5m', target: 2000 },
        { duration: '1m', target: 0 },
    ],
    thresholds: {
        http_req_duration: ['p(99)<1000'],
        http_req_failed: ['rate<0.05'],
    },
};
```

#### 3. Stress Test
```javascript
export let options = {
    stages: [
        { duration: '2m', target: 5000 },
        { duration: '5m', target: 5000 },
        { duration: '1m', target: 0 },
    ],
    maxVUs: 6000,
};
```

### Test Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/v1/scrape` | POST | Create scrape task |
| `/v1/scrape/{taskId}` | GET | Get task status |
| `/v1/search` | POST | Search (if configured) |

### Thresholds

| Metric | Threshold | Description |
|--------|-----------|-------------|
| `http_req_duration` | p(95) < 500ms | 95% of requests complete under 500ms |
| `http_req_failed` | rate < 0.01 | Less than 1% error rate |

### Running in Docker

```bash
# Using k6 Docker image
docker run -i loadimpact/k6 run - <tests/stress/k6_script.js
```

### Environment Variables

```bash
export BASE_URL=http://localhost:8899
export API_KEY=your_test_api_key
```

### Expected Results

| Load Level | Target RPS | P95 Latency | Error Rate |
|------------|------------|-------------|------------|
| 50 users | ~100 RPS | < 100ms | < 0.1% |
| 200 users | ~400 RPS | < 200ms | < 0.5% |
| 500 users | ~800 RPS | < 300ms | < 1% |
| 1000 users | ~1500 RPS | < 500ms | < 2% |

### Monitoring

During load tests, monitor:

1. **Prometheus Metrics**:
   - `http_requests_total`
   - `http_request_duration_seconds`
   - `task_queue_depth`

2. **System Resources**:
   - CPU usage
   - Memory usage
   - Network I/O

3. **Database**:
   - PostgreSQL connections
   - Redis memory
   - Query latency

### CI/CD Integration

```yaml
# GitHub Actions Example
- name: Run Load Tests
  if: github.ref == 'refs/heads/main'
  run: |
    k6 run tests/stress/k6_script.js \
      --out json=load-test-results.json \
      --summary-trendstats="avg,min,med,max,p(95),p(99)"
```

### Troubleshooting

1. **High Latency**
   - Check database connection pool
   - Review Redis performance
   - Check for resource contention

2. **High Error Rate**
   - Review rate limiting configuration
   - Check worker pool size
   - Verify external service connectivity

3. **Memory Issues**
   - Reduce batch sizes
   - Increase worker count
   - Check for memory leaks

### Recommendations

1. **Pre-Test**: Run baseline tests before code changes
2. **Post-Test**: Compare results to detect regressions
3. **Regular Testing**: Run weekly or after major changes
4. **Environment**: Use production-like data volumes
