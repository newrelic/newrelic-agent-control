# Agent Control Diagnostics System

Sistema completo de profiling y diagnostics para detectar memory leaks, spawns que no se cierran, y hacer CPU profiling on-demand.

## 🚀 Compilación

### Compilar con todas las features de diagnostics:

```bash
cargo build --release --features diagnostics
```

### Compilar solo con features específicas:

```bash
# Solo CPU profiling
cargo build --release --features pprof

# Solo tokio-console
cargo build --release --features tokio-console

# Solo heap profiling
cargo build --release --features dhat-heap
```

## 📊 Endpoints HTTP Disponibles

Una vez que Agent Control está corriendo con diagnostics habilitado, estos endpoints están disponibles:

### Health Check & Status

```bash
# Health check con información básica
curl http://localhost:8080/diagnostics/health

# Snapshot completo de diagnostics
curl http://localhost:8080/diagnostics
```

### Spawn Tracking (Detectar Leaks)

```bash
# Ver todos los spawns activos
curl http://localhost:8080/diagnostics/spawns/active

# Ver spawns completados (últimos 100)
curl http://localhost:8080/diagnostics/spawns/completed?limit=100

# Detectar potential memory leaks (spawns corriendo más de 5 minutos)
curl http://localhost:8080/diagnostics/spawns/leaks?threshold_secs=300

# Limpiar historial de spawns completados
curl -X DELETE http://localhost:8080/diagnostics/spawns/history
```

### CPU Profiling

```bash
# Iniciar profiling (100 Hz de frecuencia)
curl -X POST http://localhost:8080/diagnostics/profiling/start \
  -H "Content-Type: application/json" \
  -d '{"frequency": 100, "session_name": "investigation-leak"}'

# Ver status del profiling
curl http://localhost:8080/diagnostics/profiling/status

# Detener y generar flamegraph
curl -X POST http://localhost:8080/diagnostics/profiling/stop \
  -H "Content-Type: application/json" \
  -d '{"format": "flamegraph", "output_path": "/tmp/my-flamegraph.svg"}'

# Detener y generar pprof (para Google pprof)
curl -X POST http://localhost:8080/diagnostics/profiling/stop \
  -H "Content-Type: application/json" \
  -d '{"format": "pprof", "output_path": "/tmp/profile.pb"}'
```

## 🔧 Uso en Código

### Inicializar el tracker global al startup:

```rust
use newrelic_agent_control::diagnostics::global::init_global_tracker;

// En tu main o init function
fn main() {
    // Inicializar con historial de 10,000 spawns completados
    let tracker = init_global_tracker(10000);

    // Pasar el tracker al HTTP server
    run_status_server(
        server_config,
        event_consumer,
        sub_agent_consumer,
        opamp_config,
        startup_publisher,
        Some(DiagnosticsConfig::default()),
        Some(tracker),
    );
}
```

### Usar spawn tracking en tu código:

```rust
use newrelic_agent_control::spawn_global;

// Opción 1: Usar el macro (recomendado)
spawn_global!("my-task-name", async {
    // Tu código async aquí
    process_something().await;
});

// Opción 2: Usar el tracker directamente
use newrelic_agent_control::diagnostics::global::global_tracker;

if let Some(tracker) = global_tracker() {
    tracker.spawn_tracked(
        "my-task",
        file!(),
        line!(),
        async {
            // Tu código
        }
    );
}
```

## 🐛 Workflow para Detectar Memory Leaks

### 1. Port-forward del pod (Kubernetes)

```bash
kubectl port-forward -n newrelic-agent-control \
    deployment/agent-control 8080:8080
```

### 2. Verificar spawns activos iniciales

```bash
curl http://localhost:8080/diagnostics/spawns/active | jq '.total_active'
```

### 3. Dejar correr por un tiempo mientras reproduces el problema

Por ejemplo, si sospechas un leak al procesar ciertos eventos, reproduce esos eventos.

### 4. Detectar spawns que no se cierran (potenciales leaks)

```bash
# Spawns corriendo más de 5 minutos
curl "http://localhost:8080/diagnostics/spawns/leaks?threshold_secs=300" | jq

# Output example:
# {
#   "potential_leaks": [
#     {
#       "id": "abc-123",
#       "name": "opamp-client-connection",
#       "spawned_at": "...",
#       "location": "src/opamp/client.rs:45",
#       "status": "Running"
#     }
#   ],
#   "count": 1,
#   "threshold_secs": 300
# }
```

### 5. Hacer CPU profiling para ver dónde se gasta tiempo

```bash
# Iniciar profiling
curl -X POST http://localhost:8080/diagnostics/profiling/start \
  -H "Content-Type: application/json" \
  -d '{"frequency": 1000}'

# Dejar correr 30-60 segundos mientras reproduces el problema

# Detener y generar flamegraph
curl -X POST http://localhost:8080/diagnostics/profiling/stop \
  -H "Content-Type: application/json" \
  -d '{"format": "flamegraph"}'

# Copiar el flamegraph desde el pod
kubectl cp newrelic-agent-control/agent-control-xxxxx:/tmp/flamegraph_*.svg ./flamegraph.svg

# Abrir en navegador
open flamegraph.svg
```

### 6. Analizar el flamegraph

El flamegraph te mostrará:
- Qué funciones consumen más CPU
- Call stacks completos
- Dónde están los hot paths

Busca patrones como:
- Loops infinitos
- Funciones que no deberían estar activas
- Spawns que se quedaron en await

## 🎯 Tips para Detectar Spawns que No se Cierran

### Patrón 1: Spawns que crecen linealmente

```bash
# Check cada 30 segundos
watch -n 30 'curl -s http://localhost:8080/diagnostics/spawns/active | jq ".total_active"'

# Si el número sigue creciendo y nunca baja, tienes un leak
```

### Patrón 2: Ver los spawns más viejos

```bash
curl http://localhost:8080/diagnostics/spawns/active | \
  jq '.active_tasks | sort_by(.spawned_at) | .[0:5]'

# Si ves spawns con el mismo nombre que llevan horas corriendo,
# probablemente hay un leak
```

### Patrón 3: Buscar spawns de un tipo específico

```bash
curl http://localhost:8080/diagnostics/spawns/active | \
  jq '.active_tasks[] | select(.name | contains("opamp"))'

# Filtra por nombre para enfocarte en componentes específicos
```

## 🔍 Tokio Console (para async debugging avanzado)

Si compilaste con `--features tokio-console`:

```bash
# Port-forward puerto de tokio-console
kubectl port-forward -n newrelic-agent-control \
    deployment/agent-control 6669:6669

# Instalar tokio-console
cargo install --locked tokio-console

# Conectar
tokio-console http://localhost:6669
```

Tokio Console te muestra:
- ✅ Todos los tasks en tiempo real
- ✅ Cuánto tiempo cada task está idle vs running
- ✅ Qué tasks están bloqueando el runtime
- ✅ Dónde fueron spawneados los tasks

## 📝 Habilitar en Kubernetes

### Opción 1: Variable de entorno

Agrega al deployment:

```yaml
env:
- name: ENABLE_DIAGNOSTICS
  value: "true"
```

### Opción 2: Compilar con la feature

Modifica el Dockerfile:

```dockerfile
RUN cargo build --release --features diagnostics
```

### Exponer puertos

```yaml
ports:
- name: http
  containerPort: 8080
- name: tokio-console  # Solo si usas tokio-console
  containerPort: 6669
```

## 🚨 Casos de Uso Comunes

### Memory Leak en OpAMP Client

```bash
# 1. Ver spawns relacionados con opamp
curl http://localhost:8080/diagnostics/spawns/active | jq '.active_tasks[] | select(.name | contains("opamp"))'

# 2. Si hay muchos, iniciar profiling
curl -X POST http://localhost:8080/diagnostics/profiling/start -H "Content-Type: application/json" -d '{"frequency": 1000}'

# 3. Esperar 60 segundos

# 4. Generar flamegraph
curl -X POST http://localhost:8080/diagnostics/profiling/stop -H "Content-Type: application/json" -d '{"format": "flamegraph"}'
```

### Spawn que No Se Cierra

Si ves un spawn específico que no se cierra:

1. Nota el `location` (archivo:línea)
2. Ve al código en esa ubicación
3. Revisa:
   - ¿Hay un loop infinito?
   - ¿Falta un `break` o `return`?
   - ¿Hay un `select!` que nunca completa?
   - ¿Hay un channel que nunca recibe un mensaje de cierre?

### Performance Degradation

```bash
# 1. Snapshot inicial
curl http://localhost:8080/diagnostics > before.json

# 2. Reproduce el problema

# 3. Snapshot después
curl http://localhost:8080/diagnostics > after.json

# 4. Comparar
diff <(jq . before.json) <(jq . after.json)
```

## 🎓 Interpretación de Métricas

### spawn_stats

```json
{
  "total_spawned": 1000,      // Total spawns desde inicio
  "total_completed": 995,     // Total completados
  "currently_active": 5,      // Activos ahora
  "completed_history_size": 100
}
```

**Señal de leak**: `total_spawned - total_completed` crece con el tiempo.

### runtime

```json
{
  "num_workers": 4,             // Threads en runtime
  "active_tasks_count": 10,     // Tasks en runtime de tokio
  "spawned_tasks_count": 1000   // Total spawned por tokio
}
```

### memory

```json
{
  "physical_mem_bytes": "123 MB",
  "virtual_mem_bytes": "456 MB"
}
```

**Señal de leak**: Memoria crece linealmente con el tiempo sin estabilizarse.

## ⚠️ Notas de Performance

- Spawn tracking tiene overhead mínimo (~microsegundos por spawn)
- CPU profiling solo corre cuando lo activas explícitamente
- Tokio console tiene overhead moderado (~5-10%), solo úsalo en desarrollo
- Para producción, compila sin `tokio-console` feature

## 🔐 Seguridad

Los endpoints de diagnostics NO tienen autenticación por defecto. En producción:

1. Usa network policies para limitar acceso
2. O agrega middleware de autenticación
3. O expón solo en localhost/internal network

## 📚 Referencias

- [pprof](https://github.com/tikv/pprof-rs)
- [tokio-console](https://github.com/tokio-rs/console)
- [dhat](https://github.com/nnethercote/dhat-rs)
- [Tokio metrics](https://docs.rs/tokio/latest/tokio/runtime/struct.RuntimeMetrics.html)
