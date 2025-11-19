# CoW Protocol Kubernetes Deployment

Simple Kubernetes deployment for CoW Protocol services.

## Architecture

- **Orderbook** - Order management API (port 8080)
- **Autopilot** - Automated settlement service
- **Driver** - Settlement driver
- **Baseline** - Baseline solver
- **DB Migration Job** - Database setup

## Prerequisites

1. Kubernetes cluster (v1.20+)
2. kubectl configured
3. Docker images in your registry
4. External PostgreSQL database

## Quick Start

### 1. Update Configuration

Edit [01-config.yaml](01-config.yaml) and update:

```yaml
# Database - Update these
DB_WRITE_URL: "postgres://your-db-host:5432/cow_protocol?user=postgres&password=yourpassword"
DB_READ_URL: "postgres://your-db-host:5432/cow_protocol?user=postgres&password=yourpassword"

# RPC endpoints - Update these
ETH_RPC_URL: "wss://your-rpc-endpoint"
NODE_URL: "wss://your-rpc-endpoint"
SIMULATION_NODE_URL: "wss://your-rpc-endpoint"

# Solver account - Update this
SOLVER_ACCOUNT: "your-private-key"
```

### 2. Update Container Images

Replace `YOUR_REGISTRY` in all deployment files:
- [02-db-migration-job.yaml](02-db-migration-job.yaml)
- [03-orderbook.yaml](03-orderbook.yaml)
- [04-autopilot.yaml](04-autopilot.yaml)
- [05-driver.yaml](05-driver.yaml)
- [06-baseline.yaml](06-baseline.yaml)

Example:
```yaml
image: myregistry.io/cow-protocol-orderbook:latest
```

### 3. Deploy

```bash
# Apply all manifests at once (recommended - init containers handle dependencies)
kubectl apply -f deploy/k8s/

# Wait for migration to complete
kubectl wait --for=condition=complete --timeout=300s job/db-migrations -n cow-protocol

# Check status
kubectl get pods -n cow-protocol
```

Or deploy in order manually:

```bash
# Step 1: Namespace and config
kubectl apply -f deploy/k8s/00-namespace.yaml
kubectl apply -f deploy/k8s/01-config.yaml

# Step 2: Run database migration
kubectl apply -f deploy/k8s/02-db-migration-job.yaml
kubectl wait --for=condition=complete --timeout=300s job/db-migrations -n cow-protocol

# Step 3: Deploy orderbook (depends on migration)
kubectl apply -f deploy/k8s/03-orderbook.yaml
kubectl wait --for=condition=available --timeout=300s deployment/orderbook -n cow-protocol

# Step 4: Deploy driver and baseline (no dependencies)
kubectl apply -f deploy/k8s/05-driver.yaml
kubectl apply -f deploy/k8s/06-baseline.yaml

# Step 5: Deploy autopilot (depends on orderbook via init container)
kubectl apply -f deploy/k8s/04-autopilot.yaml
```

## Service Dependencies

The deployment includes init containers that manage service dependencies (similar to docker-compose `depends_on`):

**Dependency Graph:**
```
DB Migration Job
      ↓
  Orderbook
      ↓
  Autopilot   (Driver + Baseline start in parallel, no dependencies)
```

**How it works:**
- **Orderbook**: Starts immediately after namespace and config are applied
- **Autopilot**: Has init container that waits for orderbook service port 80 to accept connections
- **Driver & Baseline**: No dependencies, start immediately in parallel

**Init Containers:**
- Use `busybox` image with `nc -z` (netcat) to check if service port is accepting connections
- Wait in a loop until the service port responds
- Only then allow the main container to start

**Note:** Services do not have health check endpoints, so we use TCP port checks (nc -z) to verify service availability instead of readiness/liveness probes.

## Build Docker Images

```bash
cd playground

# Build and push all images
docker build -t YOUR_REGISTRY/cow-protocol-migrations:latest --target migrations -f Dockerfile ..
docker push YOUR_REGISTRY/cow-protocol-migrations:latest

docker build -t YOUR_REGISTRY/cow-protocol-orderbook:latest --target production -f Dockerfile ..
docker push YOUR_REGISTRY/cow-protocol-orderbook:latest

docker build -t YOUR_REGISTRY/cow-protocol-autopilot:latest --target production -f Dockerfile ..
docker push YOUR_REGISTRY/cow-protocol-autopilot:latest

docker build -t YOUR_REGISTRY/cow-protocol-driver:latest --target production -f Dockerfile ..
docker push YOUR_REGISTRY/cow-protocol-driver:latest

docker build -t YOUR_REGISTRY/cow-protocol-baseline:latest --target production -f Dockerfile ..
docker push YOUR_REGISTRY/cow-protocol-baseline:latest
```

## Accessing Services

### Port Forward

```bash
# Orderbook API
kubectl port-forward -n cow-protocol svc/orderbook-service 8080:80

# Metrics
kubectl port-forward -n cow-protocol svc/orderbook-service 9586:9586
kubectl port-forward -n cow-protocol svc/autopilot-service 9589:9589
```

Access at:
- Orderbook: http://localhost:8080
- Metrics: http://localhost:9586/metrics

### Expose via Ingress

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: cow-protocol
  namespace: cow-protocol
spec:
  rules:
  - host: api.yourdomain.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: orderbook-service
            port:
              number: 80
```

## Scaling

```bash
# Scale services
kubectl scale deployment orderbook -n cow-protocol --replicas=3
kubectl scale deployment autopilot -n cow-protocol --replicas=2

# Auto-scaling
kubectl autoscale deployment orderbook -n cow-protocol --cpu-percent=70 --min=2 --max=10
```

## Monitoring

All services expose Prometheus metrics:
- Orderbook: port 9586
- Autopilot: port 9589

## Troubleshooting

### Check Logs

```bash
kubectl logs -f deployment/orderbook -n cow-protocol
kubectl logs -f deployment/autopilot -n cow-protocol
kubectl logs -f deployment/driver -n cow-protocol
kubectl logs -f deployment/baseline -n cow-protocol
```

### Check Events

```bash
kubectl get events -n cow-protocol --sort-by='.lastTimestamp'
```

### Describe Resources

```bash
kubectl describe pod <pod-name> -n cow-protocol
kubectl describe deployment <deployment-name> -n cow-protocol
```

## Cleanup

```bash
# Delete everything
kubectl delete namespace cow-protocol
```

## Production Tips

1. **Secrets**: Use external secret managers (Vault, AWS Secrets Manager, etc.)
2. **Resources**: Adjust CPU/memory limits based on load
3. **Monitoring**: Set up Prometheus/Grafana
4. **Logging**: Use centralized logging (ELK, Loki)
5. **Backup**: Regular database backups
6. **SSL/TLS**: Configure at ingress level
