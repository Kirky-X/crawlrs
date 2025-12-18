import http from 'k6/http';
import { check, sleep } from 'k6';

export let options = {
    stages: [
        { duration: '30s', target: 50 },   // Ramp up to 50 users
        { duration: '1m', target: 200 },  // Ramp up to 200 users
        { duration: '1m', target: 500 },  // Ramp up to 500 users
        { duration: '2m', target: 1000 }, // Ramp up to 1000 users
        { duration: '2m', target: 1000 }, // Stay at 1000 users
        { duration: '30s', target: 0 },    // Ramp down to 0
    ],
    thresholds: {
        http_req_duration: ['p(95)<500'], // 95% of requests must complete below 500ms
        http_req_failed: ['rate<0.01'],   // http errors should be less than 1%
    },
};

const BASE_URL = 'http://localhost:3000';

export default function () {
    // 1. Health Check
    let res = http.get(`${BASE_URL}/health`);
    check(res, { 'health status was 200': (r) => r.status === 200 });

    // 2. Create Scrape Task
    const payload = JSON.stringify({
        url: `https://example.com/page-${Math.floor(Math.random() * 100000)}`,
        formats: ["html", "text"],
    });

    const params = {
        headers: {
            'Content-Type': 'application/json',
        },
    };

    res = http.post(`${BASE_URL}/v1/scrape`, payload, params);
    
    const isCreateSuccess = check(res, {
        'create scrape status was 200 or 201': (r) => r.status === 200 || r.status === 201,
    });

    if (isCreateSuccess) {
        let taskId;
        try {
            const body = JSON.parse(res.body);
            taskId = body.id;
        } catch (e) {
            console.error('Failed to parse response body: ' + res.body);
            return;
        }

        if (taskId) {
            // 3. Check Task Status
            sleep(Math.random() * 2 + 1); // Wait 1-3 seconds
            
            res = http.get(`${BASE_URL}/v1/scrape/${taskId}`);
            check(res, {
                'get status status was 200': (r) => r.status === 200,
                'task has status': (r) => {
                    try {
                        return JSON.parse(r.body).status !== undefined;
                    } catch (e) {
                        return false;
                    }
                },
            });
        }
    }

    sleep(1);
}
