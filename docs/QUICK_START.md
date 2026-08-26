# ESA Demo - Quick Start

## 🚀 Start All Services

You have **3 services** running:

1. **Backend API** (Rust) - Port 8080 ✅
2. **Payment Simulator** (Next.js) - Port 3000 ✅  
3. **ESA Control Plane** (React) - Port 3001 ✅

## 📖 Demo Flow (5 Minutes)

### Step 1: Seed Data (30 seconds)

Open: **http://localhost:3000** (Payment Simulator)

1. Click **"Seed Demo Data"** button
2. Wait for success message
3. You should see 3 workloads appear:
   - payment-upi-india-south
   - payment-cards-india-west
   - payment-netbanking-india-north

### Step 2: Verify ESA Control Plane (30 seconds)

Open: **http://localhost:3001** (ESA Control Plane)

**Check all 4 tabs:**

✅ **Command Center (Dashboard)**
- Should show 3 workloads
- All in HEALTHY state
- Metrics displaying

✅ **Agents**
- 4 agent cards: Monitor, Diagnosis, Planning, Safety
- All showing "active" status
- Current tasks displayed

✅ **Runtime**
- Regional distribution showing 3 regions
- Workload topology with details
- Replica counts

✅ **Audit**
- Shows 2 mock actions (or empty if fresh start)
- Will populate when you trigger spike

### Step 3: Trigger Traffic Spike (1 minute)

Back to: **http://localhost:3000** (Payment Simulator)

1. Set slider to **3x multiplier**
2. Click **"Trigger Traffic Spike"**
3. Watch metrics turn RED:
   - P95 Latency: 120ms → 380ms 🔴
   - Error Rate: 1% → 5% 🔴
   - Queue Depth: 500 → 2,500 🔴
   - Status: DEGRADED ⚠️

### Step 4: Watch ESA Respond (2 minutes)

Switch to: **http://localhost:3001** (ESA Control Plane)

**Dashboard Tab:**
- Workload state changes to DEGRADED
- Latency metrics spiking

**Agents Tab:**
- Monitor Agent: Should show "Observing workload metrics"
- Other agents: Active and processing

**Audit Tab:**
- New actions appearing (if orchestrator cycle runs)

### Step 5: Observe Recovery (1 minute)

Back to: **http://localhost:3000** (Payment Simulator)

- Metrics gradually improving (if ESA takes action)
- Or manually refresh after 20 seconds

**Expected Results:**
- ✅ Latency drops back to ~110-140ms
- ✅ Error rate returns to ~1%
- ✅ Queue depth reduces
- ✅ Status: HEALTHY

## 🎯 What You're Demonstrating

### The Two-UI Story:

**UI #1 (Port 3000) = Payment Event Surface**
- Shows the "problem" - traffic spike causing degradation
- Simulates payment infrastructure metrics
- Represents what Razorpay engineers would see

**UI #2 (Port 3001) = ESA Control Plane**
- Shows the "solution" - AI agents + policy engine working
- Demonstrates autonomous decision-making
- Proves everything is auditable and policy-bounded

### Key Points for Judges:

1. **Problem**: Traffic spike (3x load) causes latency and errors
2. **Detection**: Monitor Agent sees the degradation automatically
3. **Diagnosis**: AI analyzes the cause (hot partition, capacity issue)
4. **Planning**: Proposes action (CREATE_REPLICA, SHIFT_ROUTE)
5. **Safety**: Reviews proposal against constitutional rules
6. **Policy**: Validates against hard limits (replica max, risk level)
7. **Execution**: Action Gateway executes safely
8. **Recovery**: Metrics return to healthy in ~20 seconds
9. **Audit**: Complete trail of decisions captured

## 🔧 Troubleshooting

### No Data in ESA Control Plane?
```bash
# Check backend is running
curl http://localhost:8080/health

# Should return: {"service":"esa-api","status":"healthy"}
```

### Agents Not Showing?
```bash
# Check agents endpoint
curl http://localhost:8080/api/agents/status | jq '.agents | length'

# Should return: 4
```

### Workloads Not Loading?
```bash
# Seed data manually
curl -X POST http://localhost:8080/api/demo/seed

# Verify workloads
curl http://localhost:8080/api/workloads | jq '. | length'

# Should return: 3
```

### Frontend Not Updating?
- Check browser console for errors
- Verify CORS is working (should see no errors)
- Hard refresh: Cmd+Shift+R (Mac) or Ctrl+Shift+R (Windows)

## 📱 URLs Reference

| Service | URL | Purpose |
|---------|-----|---------|
| Payment Simulator | http://localhost:3000 | Generate traffic spikes |
| ESA Control Plane | http://localhost:3001 | Watch agent decisions |
| Backend API | http://localhost:8080 | REST API endpoints |
| Health Check | http://localhost:8080/health | Verify backend |
| Workloads API | http://localhost:8080/api/workloads | Get workloads |
| Agents API | http://localhost:8080/api/agents/status | Get agent status |

## 🎬 Demo Script

See [DEMO_GUIDE.md](DEMO_GUIDE.md) for the complete 2-minute pitch to judges.

## 🚨 Important Notes

- ⚠️ This simulates payment infrastructure, not real payments
- ⚠️ Not connected to Razorpay production
- ⚠️ AI agent actions are policy-bounded (safe by default)
- ⚠️ Everything is auditable - complete decision lineage

---

**Ready to rock your Razorpay demo! 🚀**
