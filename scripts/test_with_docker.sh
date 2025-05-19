#!/usr/bin/env bash
#
# scripts/test_with_docker.sh
# ------------------------------------
# Utility script to spin-up disposable Docker containers for
# PostgreSQL, MySQL and Redis, export the expected *_TEST_URL
# environment variables and then run the full Rust test suite.
#
# This makes it trivial to run the SDK & orchestrator integration
# tests locally or on CI without permanent service installations.
#
set -euo pipefail

POSTGRES_CONTAINER=pywatt-test-postgres
MYSQL_CONTAINER=pywatt-test-mysql
REDIS_CONTAINER=pywatt-test-redis
NETWORK=pywatt-test-net
POSTGRES_IMAGE=postgres:15
MYSQL_IMAGE=mysql:8
REDIS_IMAGE=redis:7

# Track test statuses
SQLITE_STATUS="Not Run"
POSTGRES_STATUS="Not Run"
MYSQL_APPLY_STATUS="Not Run"
MYSQL_SYNC_STATUS="Not Run"
MYSQL_ENUM_STATUS="Not Run"
START_TIME=$(date +%s)

# ---------- Helper functions ----------
cleanup() {
  echo "Shutting down test containers …"
  docker rm -f "$POSTGRES_CONTAINER" "$MYSQL_CONTAINER" "$REDIS_CONTAINER" >/dev/null 2>&1 || true
  docker network rm "$NETWORK" >/dev/null 2>&1 || true
}

wait_for_port() {
  local host=$1 port=$2
  echo -n "Waiting for $host:$port … "
  for _ in {1..40}; do
    if command -v nc >/dev/null 2>&1; then
      if nc -z "$host" "$port"; then echo "up!"; return 0; fi
    else
      # Fallback using bash /dev/tcp when nc is unavailable
      if (echo > /dev/tcp/$host/$port) >/dev/null 2>&1; then echo "up!"; return 0; fi
    fi
    sleep 2
  done
  echo "timeout!" >&2
  exit 1
}

# Get mapped host port for a given container internal port
get_host_port() {
  local container=$1 internal=$2
  docker port "$container" "$internal/tcp" | head -n1 | awk -F: '{print $2}'
}

# Display summary of test results
display_summary() {
  END_TIME=$(date +%s)
  DURATION=$((END_TIME - START_TIME))
  MINS=$((DURATION / 60))
  SECS=$((DURATION % 60))
  
  echo
  echo "==========================================================="
  echo "                 TEST EXECUTION SUMMARY                    "
  echo "==========================================================="
  echo "Total execution time: ${MINS}m ${SECS}s"
  echo
  echo "SQLite Tests:             $SQLITE_STATUS"
  echo "PostgreSQL Tests:         $POSTGRES_STATUS"
  echo "MySQL Apply/Sync Test:    $MYSQL_APPLY_STATUS"
  echo "MySQL Idempotency Test:   $MYSQL_SYNC_STATUS"
  echo "MySQL Enum/Constraints:   $MYSQL_ENUM_STATUS"
  echo "==========================================================="
  
  # Check if any test failed
  if [[ "$SQLITE_STATUS" == *"FAIL"* ]] || 
     [[ "$POSTGRES_STATUS" == *"FAIL"* ]] || 
     [[ "$MYSQL_APPLY_STATUS" == *"FAIL"* ]] || 
     [[ "$MYSQL_SYNC_STATUS" == *"FAIL"* ]] || 
     [[ "$MYSQL_ENUM_STATUS" == *"FAIL"* ]]; then
    echo "❌ Some tests FAILED!"
    return 1
  else
    echo "✅ All tests PASSED!"
    return 0
  fi
}

# Run a test and update its status
run_test() {
  local test_name=$1
  local test_cmd=$2
  local status_var=$3
  
  echo -e "\nRunning $test_name..."
  if $test_cmd; then
    eval "$status_var='✅ PASS'"
  else
    eval "$status_var='❌ FAIL'"
  fi
}

# Clean up any previous container instances before starting
echo "Cleaning up any existing containers with the same names..."
docker rm -f "$POSTGRES_CONTAINER" "$MYSQL_CONTAINER" "$REDIS_CONTAINER" >/dev/null 2>&1 || true
docker network rm "$NETWORK" >/dev/null 2>&1 || true

# Register the cleanup handler
trap cleanup EXIT

echo "Creating isolated Docker network …"
docker network create "$NETWORK" >/dev/null 2>&1 || true

echo "Starting PostgreSQL …"
docker run -d --name "$POSTGRES_CONTAINER" --network "$NETWORK" -e POSTGRES_PASSWORD=postgres \
           -e POSTGRES_DB=pywatt_test -P "$POSTGRES_IMAGE" > /dev/null

POSTGRES_HOST_PORT=$(get_host_port "$POSTGRES_CONTAINER" 5432)

echo "Starting MySQL …"
# Use a simpler MySQL setup with fewer variables
docker run -d --name "$MYSQL_CONTAINER" --network "$NETWORK" \
           -e MYSQL_ROOT_PASSWORD=mysql \
           -e MYSQL_DATABASE=pywatt_test \
           -e MYSQL_ROOT_HOST='%' \
           -P "$MYSQL_IMAGE" > /dev/null

MYSQL_HOST_PORT=$(get_host_port "$MYSQL_CONTAINER" 3306)

echo "Starting Redis …"
docker run -d --name "$REDIS_CONTAINER" --network "$NETWORK" -P "$REDIS_IMAGE" > /dev/null

REDIS_HOST_PORT=$(get_host_port "$REDIS_CONTAINER" 6379)

# Show container status
echo "Docker container status:"
docker ps

# ---------- Wait for services ----------
wait_for_port 127.0.0.1 "$POSTGRES_HOST_PORT"
wait_for_port 127.0.0.1 "$MYSQL_HOST_PORT"
wait_for_port 127.0.0.1 "$REDIS_HOST_PORT"

# Print MySQL logs to diagnose any issues
echo "MySQL container logs:"
docker logs "$MYSQL_CONTAINER"

# Give services extra time to fully initialize (especially important for MySQL)
echo "Giving MySQL extra time to complete initialization..."
sleep 15

# Try a basic connection test to MySQL
echo "Testing MySQL connection..."
if docker run --rm --network="$NETWORK" "$MYSQL_IMAGE" \
   mysql --host="$MYSQL_CONTAINER" --user=root --password=mysql \
   --connect-timeout=5 -e "SELECT 1;" >/dev/null 2>&1; then
  echo "MySQL connection successful!"
else
  echo "MySQL connection failed. Container may not be fully initialized."
  echo "Proceeding with tests anyway..."
fi

# ---------- Export connection URLs expected by integration tests ----------
export POSTGRES_TEST_URL="postgres://postgres:postgres@127.0.0.1:${POSTGRES_HOST_PORT}/pywatt_test"
# Add connection pool and timeout settings to MySQL URL
export MYSQL_TEST_URL="mysql://root:mysql@127.0.0.1:${MYSQL_HOST_PORT}/pywatt_test?pool_timeout=60&connect_timeout=60"
export REDIS_TEST_URL="redis://127.0.0.1:${REDIS_HOST_PORT}"

echo "Connection URLs:"
echo "POSTGRES_TEST_URL=$POSTGRES_TEST_URL"
echo "MYSQL_TEST_URL=$MYSQL_TEST_URL"
echo "REDIS_TEST_URL=$REDIS_TEST_URL"

# Run SQLite tests
run_test "SQLite tests" "cargo test --all-features model_manager::integration_tests::tests::test_sqlite -- --include-ignored" SQLITE_STATUS

# Run PostgreSQL tests 
run_test "PostgreSQL tests" "cargo test --all-features model_manager::integration_tests::tests::test_postgres -- --include-ignored" POSTGRES_STATUS

echo -e "\nRunning MySQL tests individually for better isolation..."

# Run each MySQL test in isolation
run_test "MySQL Apply and Sync test" "cargo test --all-features model_manager::integration_tests::tests::test_mysql_apply_and_sync_new_model -- --include-ignored" MYSQL_APPLY_STATUS

run_test "MySQL Sync Idempotency test" "cargo test --all-features model_manager::integration_tests::tests::test_mysql_sync_idempotency_and_additive_migration -- --include-ignored" MYSQL_SYNC_STATUS

run_test "MySQL Enum and Constraints test" "cargo test --all-features model_manager::integration_tests::tests::test_mysql_enum_and_multiple_constraints -- --include-ignored" MYSQL_ENUM_STATUS

# Display summary and set exit code based on test results
display_summary
TEST_RESULT=$?

echo -e "\nAll tests finished."
exit $TEST_RESULT 