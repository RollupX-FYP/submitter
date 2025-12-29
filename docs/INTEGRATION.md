# Integration Guide

This guide details how to deploy, configure, and operate the Submitter service in a full end-to-end environment using Docker.

## 1. Docker Orchestration

The `docker-compose.yml` defines the complete topology.

### Network Topology
All services run on the `rollup-net` bridge network.
*   **L1 Node**: `l1-node` (Port 8545).
*   **Database**: `postgres` (Port 5432).
*   **Submitter**: `submitter`.
*   **Archiver**: `archiver` (Mock service).

### Shared Volumes
*   `shared-data`: Used to pass the `deployments.json` artifact from the `setup` container (which runs `deploy-test-net.ts`) to the `submitter` container.

### Startup Flow
1.  `l1-node` starts.
2.  `setup` container waits for port 8545.
3.  `setup` runs deployment script, writing `deployments.json` to the shared volume.
4.  `submitter` starts, mounts the volume, and reads the Bridge address from the JSON.

## 2. Running the Simulation

```bash
# Start all services
docker compose up --build
```

### Mock Executor Configuration
You can tune the simulated load by editing the `executor-mock` environment variables in `docker-compose.yml`:

```yaml
executor-mock:
  environment:
    - BATCH_SIZE=500        # Increase throughput
    - BATCH_TIMEOUT=2000    # Lower latency
```

## 3. Manual Verification

To verify the system state manually:

### Inspect Database
```bash
# Connect to the running Postgres container
docker exec -it <container_id> psql -U user -d submitter

# Query batches
SELECT id, status, tx_hash FROM batches;
```

### Verify L1 State
Run the verification script from the host machine:

```bash
cd contracts
npx hardhat run scripts/verify-state.ts --network localhost
```
*Note: Ensure your localhost `hardhat.config.ts` points to port 8545.*

## 4. Troubleshooting

*   **"Missing deployments.json"**: The `setup` container failed. Check logs with `docker compose logs setup`.
*   **"Blobs not supported"**: Ensure `l1-node` is running a Hardhat version that supports the `cancun` hardfork (configured in `hardhat.config.ts`).
*   **"Address in use"**: Kill any local instances of `submitter-rs` or `hardhat node` running outside Docker.
