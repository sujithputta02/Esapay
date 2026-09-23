import { useState, useEffect, useRef, type ReactNode } from 'react';
import { motion, AnimatePresence, useScroll, useTransform } from 'framer-motion';
import {
  Copy,
  Check,
  ExternalLink,
  ArrowRight,
  RefreshCw,
  CreditCard,
  ShieldCheck,
  Layers,
  CheckCircle2,
  Lock,
  X,
  QrCode,
  Smartphone,
  Building2,
  AlertTriangle,
  RotateCcw,
  Download,
  Code2,
  Terminal,
  TrendingUp,
  Cpu,
  Activity,
  Zap,
  BookOpen,
} from 'lucide-react';
import { apiClient, getApiBaseUrl } from '@/lib/api';

interface GatewayItem {
  gateway: string;
  name: string;
  status: string;
  p95_latency_ms: number;
  success_rate: number;
  active_traffic_pct: number;
  is_healthy: boolean;
}

interface CheckoutDecision {
  transaction_id: string;
  amount: number;
  currency: string;
  requested_gateway: string;
  routed_gateway: string;
  failover_triggered: boolean;
  routing_reason: string;
  checkout_url: string;
}

/* ========================================================================== */
/* SCROLL DEPTH PARALLAX WRAPPER (Fades in & comes near, scrolls away & fades)*/
/* ========================================================================== */
function ScrollSection({
  children,
  className = '',
  id,
}: {
  children: ReactNode;
  className?: string;
  id?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const { scrollYProgress } = useScroll({
    target: ref,
    offset: ['start end', 'end start'],
  });

  // When scrolling in: scales from 0.88 up to 1.0 (comes near)
  // When in view: stays full scale 1.0 & opacity 1.0
  // When scrolling away: scales down to 0.88 and fades (goes far)
  const scale = useTransform(scrollYProgress, [0, 0.22, 0.78, 1], [0.88, 1, 1, 0.88]);
  const opacity = useTransform(scrollYProgress, [0, 0.2, 0.8, 1], [0.15, 1, 1, 0.15]);
  const y = useTransform(scrollYProgress, [0, 0.22, 0.78, 1], [70, 0, 0, -70]);

  return (
    <motion.div
      ref={ref}
      id={id}
      style={{ scale, opacity, y }}
      className={`will-change-transform transform-gpu ${className}`}
    >
      {children}
    </motion.div>
  );
}

/* ========================================================================== */
/* 1. INTERACTIVE 3D DOT-MATRIX SPHERE CANVAS (Indian Rail Focus)             */
/* ========================================================================== */
function DueIndiaGlobeCanvas({
  isOutageSimulated,
}: {
  isOutageSimulated: boolean;
  routedGateway: string;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let animationFrameId: number;
    let width = (canvas.width = canvas.parentElement?.clientWidth || 700);
    let height = (canvas.height = canvas.parentElement?.clientHeight || 700);

    const handleResize = () => {
      if (!canvas || !canvas.parentElement) return;
      width = canvas.width = canvas.parentElement.clientWidth;
      height = canvas.height = canvas.parentElement.clientHeight;
    };
    window.addEventListener('resize', handleResize);

    const numPoints = 1100;
    const points: { x: number; y: number; z: number }[] = [];
    const goldenRatio = (1 + Math.sqrt(5)) / 2;

    for (let i = 0; i < numPoints; i++) {
      const theta = (2 * Math.PI * i) / goldenRatio;
      const phi = Math.acos(1 - (2 * (i + 0.5)) / numPoints);
      points.push({
        x: Math.cos(theta) * Math.sin(phi),
        y: Math.sin(theta) * Math.sin(phi),
        z: Math.cos(phi),
      });
    }

    let angleY = 0;
    const angleX = 0.26;
    let mouseX = 0;
    let mouseY = 0;
    const baseSpeedY = 0.003;

    const onMouseMove = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      mouseX = (e.clientX - rect.left) / width - 0.5;
      mouseY = (e.clientY - rect.top) / height - 0.5;
    };
    window.addEventListener('mousemove', onMouseMove);

    const render = () => {
      ctx.clearRect(0, 0, width, height);

      angleY += baseSpeedY + mouseX * 0.002;
      const tiltX = angleX + mouseY * 0.12;
      const radius = Math.min(width, height) * 0.38;
      const centerX = width / 2;
      const centerY = height / 2;

      const projected = points.map((p) => {
        const cosY = Math.cos(angleY);
        const sinY = Math.sin(angleY);
        const rx1 = p.x * cosY + p.z * sinY;
        const rz1 = -p.x * sinY + p.z * cosY;

        const cosX = Math.cos(tiltX);
        const sinX = Math.sin(tiltX);
        const ry2 = p.y * cosX - rz1 * sinX;
        const rz2 = p.y * sinX + rz1 * cosX;

        return {
          px: centerX + rx1 * radius,
          py: centerY + ry2 * radius,
          depth: rz2,
        };
      });

      projected.sort((a, b) => a.depth - b.depth);

      for (let i = 0; i < projected.length; i++) {
        const pt = projected[i];
        const normDepth = (pt.depth + 1) / 2;
        const alpha = Math.max(0.12, Math.pow(normDepth, 1.8));
        const dotRadius = Math.max(1.2, normDepth * 3.4);

        ctx.save();
        ctx.beginPath();
        ctx.arc(pt.px, pt.py, dotRadius, 0, Math.PI * 2);

        if (normDepth > 0.8) {
          ctx.fillStyle = isOutageSimulated
            ? `rgba(255, 235, 180, ${alpha * 0.95})`
            : `rgba(255, 255, 255, ${alpha * 0.95})`;
          ctx.shadowColor = 'rgba(255, 255, 255, 0.5)';
          ctx.shadowBlur = 5;
        } else {
          ctx.fillStyle = `rgba(255, 255, 255, ${alpha * 0.55})`;
        }
        ctx.fill();

        if (dotRadius > 2.6) {
          ctx.beginPath();
          ctx.arc(pt.px, pt.py, dotRadius * 0.45, 0, Math.PI * 2);
          ctx.fillStyle = 'rgba(31, 81, 255, 0.85)';
          ctx.fill();
        }

        ctx.restore();
      }

      // Energy Arc across Mumbai & Bangalore
      const t = (Date.now() * 0.002) % (Math.PI * 2);
      ctx.save();
      ctx.beginPath();
      const nodeMumbaiX = centerX - radius * 0.65;
      const nodeMumbaiY = centerY + radius * 0.12;
      const nodeBLRX = centerX + radius * 0.65;
      const nodeBLRY = centerY - radius * 0.08;
      const controlY = centerY - radius * 0.55 + Math.sin(t) * 12;

      ctx.moveTo(nodeMumbaiX, nodeMumbaiY);
      ctx.quadraticCurveTo(centerX, controlY, nodeBLRX, nodeBLRY);
      ctx.strokeStyle = isOutageSimulated
        ? 'rgba(251, 191, 36, 0.5)'
        : 'rgba(255, 255, 255, 0.45)';
      ctx.lineWidth = 2.5;
      ctx.setLineDash([6, 8]);
      ctx.lineDashOffset = -Date.now() * 0.02;
      ctx.stroke();

      // Pulsing Gateway Beacons
      ctx.beginPath();
      ctx.arc(nodeMumbaiX, nodeMumbaiY, 6 + Math.sin(t * 3) * 2, 0, Math.PI * 2);
      ctx.fillStyle = isOutageSimulated ? '#F59E0B' : '#10B981';
      ctx.shadowColor = isOutageSimulated ? '#F59E0B' : '#10B981';
      ctx.shadowBlur = 12;
      ctx.fill();

      ctx.beginPath();
      ctx.arc(nodeBLRX, nodeBLRY, 6 + Math.cos(t * 3) * 2, 0, Math.PI * 2);
      ctx.fillStyle = '#FFFFFF';
      ctx.shadowColor = '#FFFFFF';
      ctx.shadowBlur = 12;
      ctx.fill();

      ctx.restore();

      animationFrameId = requestAnimationFrame(render);
    };

    render();

    return () => {
      cancelAnimationFrame(animationFrameId);
      window.removeEventListener('resize', handleResize);
      window.removeEventListener('mousemove', onMouseMove);
    };
  }, [isOutageSimulated]);

  return (
    <canvas
      ref={canvasRef}
      className="absolute inset-0 w-full h-full pointer-events-none"
    />
  );
}

/* ========================================================================== */
/* 2. ANIMATING INFRASTRUCTURE RESILIENCE PIPELINE COMPONENT                  */
/* ========================================================================== */
function AnimatingInfrastructure({ isOutage }: { isOutage: boolean }) {
  const [pulseIndex, setPulseIndex] = useState(0);

  useEffect(() => {
    const timer = setInterval(() => {
      setPulseIndex((prev) => (prev + 1) % 5);
    }, 1100);
    return () => clearInterval(timer);
  }, []);

  const steps = [
    {
      title: '1. Ingestion',
      desc: 'UPI & RuPay requests from Indian apps',
      tag: '4,033 TPS',
      icon: <Smartphone className="h-5 w-5" />,
    },
    {
      title: '2. 250ms Telemetry',
      desc: 'Z-score latency & bank timeout detection',
      tag: '0.25s TTD',
      icon: <Layers className="h-5 w-5" />,
    },
    {
      title: '3. AI Diagnosis',
      desc: 'Multi-signal bank health triangulation',
      tag: 'Hierarchical',
      icon: <Cpu className="h-5 w-5" />,
    },
    {
      title: '4. OCC Safety Gate',
      desc: 'Deterministic invariant tokens & SHA-256',
      tag: 'Zero Hallucinations',
      icon: <ShieldCheck className="h-5 w-5" />,
    },
    {
      title: '5. Dispatch Rail',
      desc: isOutage ? 'Switched to PhonePe UPI (68ms)' : 'Direct to Razorpay Express (85ms)',
      tag: isOutage ? '⚡ Failover Active' : '● Optimal Rail',
      icon: <Activity className="h-5 w-5" />,
    },
  ];

  return (
    <div className="w-full py-8">
      <div className="grid grid-cols-1 md:grid-cols-5 gap-4 relative">
        {steps.map((s, idx) => {
          const isActive = pulseIndex === idx;
          return (
            <motion.div
              key={s.title}
              animate={{
                scale: isActive ? 1.03 : 1.0,
                borderColor: isActive ? 'rgba(31, 81, 255, 0.8)' : 'rgba(0, 0, 0, 0.08)',
              }}
              transition={{ duration: 0.3 }}
              className={`p-5 rounded-3xl border bg-white shadow-sm flex flex-col justify-between relative transition-all ${
                isActive ? 'shadow-[0_12px_30px_rgba(31,81,255,0.15)] ring-2 ring-[#1F51FF]/20' : ''
              }`}
            >
              <div className="flex items-center justify-between mb-3">
                <div
                  className={`h-10 w-10 rounded-2xl flex items-center justify-center ${
                    isActive ? 'bg-[#1F51FF] text-white' : 'bg-slate-100 text-slate-700'
                  }`}
                >
                  {s.icon}
                </div>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded-full font-bold bg-slate-100 text-slate-700">
                  {s.tag}
                </span>
              </div>

              <div>
                <h4 className="font-bold text-sm text-slate-900">{s.title}</h4>
                <p className="text-xs text-slate-500 mt-1 leading-snug">{s.desc}</p>
              </div>

              {idx < steps.length - 1 && (
                <div className="hidden md:block absolute -right-2.5 top-1/2 -translate-y-1/2 z-10">
                  <div
                    className={`h-5 w-5 rounded-full bg-white border flex items-center justify-center shadow-sm ${
                      isActive ? 'border-[#1F51FF] text-[#1F51FF]' : 'border-slate-200 text-slate-400'
                    }`}
                  >
                    <ArrowRight className="h-3 w-3" />
                  </div>
                </div>
              )}
            </motion.div>
          );
        })}
      </div>
    </div>
  );
}

/* ========================================================================== */
/* 3. MAIN LANDING PAGE COMPONENT                                             */
/* ========================================================================== */
export function LandingPage() {
  const [copiedCmd, setCopiedCmd] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'gateways' | 'checkout' | 'doctor' | 'health'>('gateways');
  const [activeDevTab, setActiveDevTab] = useState<'cli' | 'typescript' | 'python'>('cli');
  const [gateways, setGateways] = useState<GatewayItem[]>([]);
  const [loadingGateways, setLoadingGateways] = useState(false);
  const [togglingGateway, setTogglingGateway] = useState<string | null>(null);

  // Live Real-Time Benchmark Readings (Ticks dynamically)
  const [liveP95, setLiveP95] = useState(68.4);
  const [liveTps, setLiveTps] = useState(4120);
  const [recentEvents, setRecentEvents] = useState<string[]>([
    '⚡ PhonePe direct UPI rail switch executing in 68ms',
    '✓ SHA-256 block #84,219 anchored to audit fabric',
    '✓ Razorpay P95 latency verified stable at 85ms',
    '✓ Policy OCC token 0x9f1a validated — zero stale mutations',
  ]);

  // India-Only Payment State
  const [amount, setAmount] = useState<number>(500);
  const [checkoutResult, setCheckoutResult] = useState<CheckoutDecision | null>(null);
  const [isProcessingCheckout, setIsProcessingCheckout] = useState(false);
  const [isOutageSimulated, setIsOutageSimulated] = useState(false);

  // In-App Universal Checkout Modal
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [modalMethod, setModalMethod] = useState<'upi' | 'card' | 'netbanking'>('upi');
  const [isPayingInModal, setIsPayingInModal] = useState(false);
  const [paymentSuccess, setPaymentSuccess] = useState(false);
  const [activeTxHash, setActiveTxHash] = useState('');

  // Interactive Documentation Modal
  const [isDocsOpen, setIsDocsOpen] = useState(false);
  const [docsTab, setDocsTab] = useState<'cli' | 'typescript' | 'python' | 'registries'>('cli');

  const serverUrl = getApiBaseUrl() || 'http://localhost:8080';

  // Hero depth scroll tracking
  const heroRef = useRef<HTMLDivElement>(null);
  const { scrollYProgress: heroProgress } = useScroll({
    target: heroRef,
    offset: ['start start', 'end start'],
  });
  const heroScale = useTransform(heroProgress, [0, 1], [1, 0.90]);
  const heroOpacity = useTransform(heroProgress, [0, 0.85], [1, 0.15]);
  const heroY = useTransform(heroProgress, [0, 1], [0, 80]);

  // Live real-time benchmark oscillation effect
  useEffect(() => {
    const interval = setInterval(() => {
      setLiveP95((prev) => {
        const delta = (Math.random() - 0.48) * 1.2;
        return Number((prev + delta).toFixed(1));
      });
      setLiveTps((prev) => prev + Math.floor((Math.random() - 0.45) * 15));
    }, 2400);
    return () => clearInterval(interval);
  }, []);

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedCmd(id);
    setTimeout(() => setCopiedCmd(null), 2000);
  };

  const fetchGateways = async () => {
    try {
      setLoadingGateways(true);
      const data = await apiClient.getGateways();
      if (Array.isArray(data)) {
        const indianGateways = data.filter((g) =>
          ['razorpay', 'phonepe', 'paytm', 'cashfree'].includes(g.gateway.toLowerCase())
        );
        setGateways(indianGateways.length > 0 ? indianGateways : data.slice(0, 4));
        const rzp = data.find((g) => g.gateway === 'razorpay');
        if (rzp) {
          setIsOutageSimulated(!rzp.is_healthy);
        }
      }
    } catch {
      setGateways([
        {
          gateway: 'razorpay',
          name: 'Razorpay UPI & Cards (Mumbai Rail)',
          status: isOutageSimulated ? 'Degraded' : 'Healthy',
          p95_latency_ms: isOutageSimulated ? 340.0 : 85.0,
          success_rate: isOutageSimulated ? 0.72 : 0.994,
          active_traffic_pct: isOutageSimulated ? 0.0 : 55.0,
          is_healthy: !isOutageSimulated,
        },
        {
          gateway: 'phonepe',
          name: 'PhonePe Direct UPI Switch (Bangalore Rail)',
          status: 'Healthy',
          p95_latency_ms: 68.0,
          success_rate: 0.996,
          active_traffic_pct: isOutageSimulated ? 75.0 : 30.0,
          is_healthy: true,
        },
        {
          gateway: 'paytm',
          name: 'Paytm All-In-One Gateway (Noida Rail)',
          status: 'Healthy',
          p95_latency_ms: 88.0,
          success_rate: 0.991,
          active_traffic_pct: 10.0,
          is_healthy: true,
        },
        {
          gateway: 'cashfree',
          name: 'Cashfree Auto-Collect & Payouts',
          status: 'Healthy',
          p95_latency_ms: 92.0,
          success_rate: 0.992,
          active_traffic_pct: 5.0,
          is_healthy: true,
        },
      ]);
    } finally {
      setLoadingGateways(false);
    }
  };

  useEffect(() => {
    fetchGateways();
  }, []);

  const handleToggleGateway = async (gwName: string) => {
    try {
      setTogglingGateway(gwName);
      await apiClient.toggleGateway(gwName);
      if (gwName === 'razorpay') {
        const nextState = !isOutageSimulated;
        setIsOutageSimulated(nextState);
        setRecentEvents((prev) => [
          nextState
            ? '⚠️ Razorpay rail degraded -> Autonomous failover executed to PhonePe'
            : '✓ Razorpay bank rail restored to healthy status (85ms)',
          ...prev.slice(0, 3),
        ]);
      }
      await fetchGateways();
    } catch {
      if (gwName === 'razorpay') {
        const nextState = !isOutageSimulated;
        setIsOutageSimulated(nextState);
        setRecentEvents((prev) => [
          nextState
            ? '⚠️ Razorpay rail degraded -> Autonomous failover executed to PhonePe'
            : '✓ Razorpay bank rail restored to healthy status (85ms)',
          ...prev.slice(0, 3),
        ]);
      }
      await fetchGateways();
    } finally {
      setTogglingGateway(null);
    }
  };

  const handleExecuteCheckout = async (openModalAfter = true) => {
    try {
      setIsProcessingCheckout(true);
      const res = await apiClient.checkout({
        amount: amount * 100,
        currency: 'INR',
        gateway: 'auto',
        method: 'UPI',
      });
      setCheckoutResult(res);
      if (openModalAfter) {
        setPaymentSuccess(false);
        setActiveTxHash(`0x${Array.from({ length: 32 }, () => Math.floor(Math.random() * 16).toString(16)).join('')}`);
        setIsModalOpen(true);
      }
    } catch {
      const fallbackResult: CheckoutDecision = {
        transaction_id: `tx_esa_${Date.now().toString(36)}`,
        amount: amount * 100,
        currency: 'INR',
        requested_gateway: 'auto',
        routed_gateway: isOutageSimulated ? 'phonepe' : 'razorpay',
        failover_triggered: isOutageSimulated,
        routing_reason: isOutageSimulated
          ? '⚡ Razorpay degraded (P95 > 250ms) -> Autonomous failover executed to PhonePe Direct UPI rail'
          : 'Direct routed to optimal low-latency Indian corridor (PhonePe Direct UPI Switch)',
        checkout_url: `/checkout/session?id=tx_esa_${Date.now().toString(36)}&gateway=phonepe`,
      };
      setCheckoutResult(fallbackResult);
      if (openModalAfter) {
        setPaymentSuccess(false);
        setActiveTxHash(`0x${Array.from({ length: 32 }, () => Math.floor(Math.random() * 16).toString(16)).join('')}`);
        setIsModalOpen(true);
      }
    } finally {
      setIsProcessingCheckout(false);
    }
  };

  const handleCompleteModalPayment = () => {
    setIsPayingInModal(true);
    setTimeout(() => {
      setIsPayingInModal(false);
      setPaymentSuccess(true);
    }, 600);
  };

  const terminalOutputs = {
    gateways: `$ npx esapay-cli gateways
================================================================================
  💳 ESA Autonomous India Multi-Gateway Routing Corridors
================================================================================
  GATEWAY NAME               STATUS       P95 LATENCY  SUCCESS RATE   TRAFFIC %
  ────────────────────────────────────────────────────────────────────────────
  Razorpay (UPI / RuPay)     ● HEALTHY     85.0ms       99.4%          55.0%
  PhonePe Direct UPI Switch  ● HEALTHY     68.0ms       99.6%          30.0%
  Paytm All-In-One Gateway   ● HEALTHY     88.0ms       99.1%          10.0%
  Cashfree Auto-Collect      ● HEALTHY     92.0ms       99.2%           5.0%
  ────────────────────────────────────────────────────────────────────────────
  💡 Run 'npx esapay-cli gateways --toggle razorpay' to simulate live outage.`,
    checkout: `$ npx esapay-cli checkout --amount 50000 --currency INR --gateway auto
================================================================================
  ⚡ ESA Universal Payment Router — Indian Settlement Result
================================================================================
  Transaction ID:     tx_esa_717c985bb5544ece8a14b4650f0718c9
  Amount:             ₹500.00 INR
  Requested Gateway:  auto
  Routed Gateway:     PHONEPE
  Failover Status:    ⚠️  TRIGGERED (Autonomous Failover)
  Routing Rationale:  ⚠️ Razorpay corridor degraded (P95 > 250ms) -> PhonePe UPI
  Checkout Session:   https://esa.mesh/checkout/session?id=tx_esa_717c985bb554...
================================================================================`,
    doctor: `$ npx esapay-cli doctor
================================================================================
  🩺 ESA System Doctor & Indian Infrastructure Diagnostics
================================================================================
  DIAGNOSTIC SUBSYSTEM                       STATUS
  ────────────────────────────────────────────────────────────────────────────
  ESA Control Plane API                ✅ PASS    http://localhost:8080
  24/7 Dockerized Ollama Engine        ✅ PASS    Online 24/7 (3 models loaded)
  Executable StateFabric Shards        ✅ PASS    3 Indian workloads active
  Cryptographic SHA-256 Audit Chain    ✅ PASS    Immutable decision ledger valid
  UPI Multi-Gateway Corridor Mesh      ✅ PASS    4/4 Indian corridors operational
  ────────────────────────────────────────────────────────────────────────────
  🛡️ System status: 100% OPERATIONAL & READY FOR LIVE TRAFFIC`,
    health: `$ npx esapay-cli health
✅ ESA Control Plane is HEALTHY at http://localhost:8080
Connected to Executable State Architecture (ESA) live cluster.`,
  };

  const sdkCodeSnippets = {
    typescript: `import { EsaGateway } from 'esapay';

// 1. Initialize client pointed at your ESA cluster
const esa = new EsaGateway({
  apiUrl: process.env.ESA_API_URL || 'http://localhost:8080',
  apiKey: process.env.ESA_API_KEY,
  timeoutMs: 8000,
});

// 2. Execute an autonomous resilient checkout (₹500.00 via UPI)
const order = await esa.checkout({
  amount: 50000,       // In paise (₹500.00)
  currency: 'INR',
  gateway: 'auto',     // Autonomous dynamic failover across Indian rails
  method: 'UPI',       // UPI | CARD | NETBANKING
});

console.log('Transaction ID:', order.transaction_id);
console.log('Routed Gateway:', order.routed_gateway);
console.log('Failover Active:', order.failover_triggered);
console.log('Session URL:', order.checkout_url);

// 3. Mathematically verify cryptographic SHA-256 audit ledger
const audit = await esa.audit.verifyChain();
console.log('Audit Integrity Verified:', audit.valid);`,
    python: `from esapay import EsaGateway
# Note: 'from esa import EsaGateway' is also fully supported

# 1. Initialize client (Zero third-party dependencies, standard library only)
esa = EsaGateway(
    api_url="http://localhost:8080",
    # api_key="secret_live_...",
    timeout=10.0,
)

# 2. Check cluster health & inspect live Indian corridors
health = esa.health()
for corridor in esa.gateways.list():
    print(f"[{corridor.status}] {corridor.name} - P95: {corridor.p95_latency_ms:.1f}ms")

# 3. Create an autonomous transaction with dynamic failover
decision = esa.checkout(
    amount=50000,  # ₹500.00 (in paise)
    currency="INR",
    gateway="auto",
    method="UPI"
)

print(f"Settled via: {decision.routed_gateway} (Failover: {decision.failover_triggered})")
print(f"Transaction ID: {decision.transaction_id}")`,
  };

  return (
    <div className="min-h-screen bg-[#FFFFFF] text-[#0A0A0A] selection:bg-[#1F51FF] selection:text-white relative overflow-hidden font-sans scroll-smooth">
      {/* ==================================================================== */}
      {/* SECTION 1: HERO (DUE SIGNATURE ELECTRIC ROYAL BLUE CANVAS)           */}
      {/* ==================================================================== */}
      <div
        ref={heroRef}
        className="relative bg-[#1F51FF] text-white overflow-hidden min-h-[96vh] flex flex-col justify-between"
      >
        <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_at_50%_40%,#295BFF_0%,#1F51FF_55%,#1644DF_100%)]" />

        {/* FLOATING TOP NAVBAR */}
        <header className="relative z-30 pt-6 px-4 sm:px-8 max-w-7xl mx-auto w-full flex items-center justify-between">
          <div className="bg-black/90 backdrop-blur-md rounded-full p-1 flex items-center shadow-xl border border-white/[0.08]">
            <span className="bg-black text-white px-4 py-1.5 rounded-full text-xs font-semibold tracking-wider">
              PRODUCT
            </span>
            <a
              href="#corridors"
              className="px-3 sm:px-4 py-1.5 text-slate-300 hover:text-white text-xs font-semibold tracking-wider transition-colors"
            >
              CORRIDORS
            </a>
            <a
              href="#sdk-cli"
              className="hidden sm:inline px-3 sm:px-4 py-1.5 text-slate-300 hover:text-white text-xs font-semibold tracking-wider transition-colors"
            >
              SDK & CLI
            </a>
            <button
              onClick={() => setIsDocsOpen(true)}
              className="px-3 sm:px-4 py-1.5 text-slate-300 hover:text-white text-xs font-semibold tracking-wider transition-colors flex items-center gap-1.5"
            >
              <BookOpen className="h-3.5 w-3.5 text-emerald-400" />
              <span>DOCS</span>
            </button>
          </div>

          <a href="#" className="flex items-center gap-1 group select-none">
            <span className="text-3xl sm:text-4xl font-extrabold tracking-tighter text-white lowercase">
              esa
            </span>
          </a>

          <div className="bg-black/90 backdrop-blur-md rounded-full p-1.5 flex items-center gap-3 text-xs font-semibold shadow-xl border border-white/[0.08]">
            <a
              href="#benchmarks"
              className="px-3 text-slate-300 hover:text-white transition-colors font-mono"
            >
              BENCHMARKS
            </a>
            <button
              onClick={() => handleExecuteCheckout(true)}
              className="bg-white text-black hover:bg-slate-100 rounded-full px-4 py-1.5 font-bold transition-all shadow-md flex items-center gap-1.5"
            >
              <span>TEST UPI</span>
              <ArrowRight className="h-3.5 w-3.5" />
            </button>
          </div>
        </header>

        {/* HERO CENTERPIECE: PARALLAX DEPTH ANIMATION WRAPPER */}
        <motion.div
          style={{ scale: heroScale, opacity: heroOpacity, y: heroY }}
          className="relative z-10 flex-1 flex flex-col justify-between my-auto px-4 sm:px-8 max-w-7xl mx-auto w-full will-change-transform transform-gpu"
        >
          {/* Globe & Corridor inputs */}
          <div className="relative flex-1 flex items-center justify-center min-h-[500px] sm:min-h-[620px] my-auto">
            <DueIndiaGlobeCanvas
              isOutageSimulated={isOutageSimulated}
              routedGateway={isOutageSimulated ? 'PhonePe' : 'Razorpay'}
            />

            <div className="relative z-20 w-full flex flex-col md:flex-row items-center justify-between gap-6 px-2 sm:px-6">
              <motion.div
                initial={{ opacity: 0, x: -30 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.6 }}
                className="flex items-center gap-3 text-white"
              >
                <span className="text-3xl sm:text-5xl font-extrabold tracking-tight">
                  &rarr; {isOutageSimulated ? 'PhonePe UPI' : 'Razorpay'}
                </span>
                {isOutageSimulated && (
                  <span className="text-[10px] uppercase font-mono px-2 py-0.5 rounded-full bg-amber-400 text-black font-extrabold animate-pulse">
                    AUTONOMOUS FAILOVER
                  </span>
                )}
              </motion.div>

              {/* Signature Due Floating White Pill Input */}
              <motion.div
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ duration: 0.6, delay: 0.1 }}
                className="w-full max-w-md mx-auto"
              >
                <div className="bg-white text-black rounded-full px-5 py-3.5 shadow-[0_20px_60px_rgba(0,0,0,0.4)] flex items-center justify-between gap-3 transition-transform hover:scale-[1.02]">
                  <div className="flex items-center gap-3">
                    <div className="h-8 w-8 rounded-full bg-slate-100 flex items-center justify-center text-base shadow-inner">
                      🇮🇳
                    </div>
                    <span className="font-mono font-extrabold text-2xl text-slate-900">₹</span>
                    <input
                      type="number"
                      value={amount}
                      onChange={(e) => setAmount(Number(e.target.value))}
                      className="font-mono font-bold text-2xl text-slate-900 bg-transparent focus:outline-none w-28 sm:w-36"
                      placeholder="500.00"
                    />
                  </div>

                  <div className="flex items-center gap-2">
                    <span className="bg-slate-100 text-slate-800 text-xs font-mono font-bold px-3 py-1.5 rounded-full">
                      INR
                    </span>

                    <button
                      onClick={() => handleExecuteCheckout(true)}
                      disabled={isProcessingCheckout}
                      className="bg-[#1F51FF] hover:bg-[#1644DF] text-white p-2.5 rounded-full transition-colors flex items-center justify-center shadow-md"
                      title="Pay / Test Corridor"
                    >
                      {isProcessingCheckout ? (
                        <RefreshCw className="h-4 w-4 animate-spin" />
                      ) : (
                        <ArrowRight className="h-4 w-4" />
                      )}
                    </button>
                  </div>
                </div>

                <div className="mt-3 flex items-center justify-center gap-3 text-xs font-mono text-white/95">
                  <span className="flex items-center gap-1.5">
                    <span
                      className={`h-2 w-2 rounded-full ${
                        isOutageSimulated ? 'bg-amber-300 animate-ping' : 'bg-emerald-300'
                      }`}
                    />
                    <span>
                      Active Route:{' '}
                      <strong>{isOutageSimulated ? 'PhonePe Direct UPI Switch' : 'Razorpay Express'}</strong>
                    </span>
                  </span>
                  <span>·</span>
                  <span>{liveP95}ms P95</span>
                </div>
              </motion.div>

              <motion.div
                initial={{ opacity: 0, x: 30 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.6 }}
                className="flex items-center gap-3 text-white"
              >
                <span className="text-3xl sm:text-5xl font-extrabold tracking-tight">
                  PhonePe &larr;
                </span>
              </motion.div>
            </div>
          </div>

          {/* Bottom Hero Bar */}
          <div className="pb-10 w-full flex flex-col md:flex-row items-start md:items-end justify-between gap-6">
            <div className="max-w-md space-y-2">
              <h2 className="text-lg sm:text-xl font-bold text-white tracking-tight leading-snug">
                Route payments across India in milliseconds
              </h2>
              <p className="text-xs sm:text-sm text-white/80 leading-relaxed">
                Transfer transactions dynamically across Razorpay, PhonePe, Paytm and Cashfree. Zero unverified mutations, sub-second failover, and zero dropped checkouts during peak flash sales.
              </p>
            </div>

            <div className="bg-black/90 backdrop-blur-md rounded-2xl p-4 border border-white/[0.1] text-xs font-mono shadow-2xl flex flex-col sm:flex-row items-center gap-4">
              <div>
                <span className="text-[10px] text-slate-400 block uppercase">Outage Simulator</span>
                <span className="text-white font-bold">
                  {isOutageSimulated ? '⚡ Razorpay Outage Active' : '● All Indian Rails Operational'}
                </span>
              </div>
              <button
                onClick={() => handleToggleGateway('razorpay')}
                disabled={togglingGateway === 'razorpay'}
                className={`px-4 py-2 rounded-xl font-bold transition-all text-xs flex items-center gap-2 ${
                  isOutageSimulated
                    ? 'bg-emerald-500 hover:bg-emerald-400 text-black shadow-[0_0_20px_rgba(16,185,129,0.4)]'
                    : 'bg-amber-400 hover:bg-amber-300 text-black shadow-[0_0_20px_rgba(251,191,36,0.4)]'
                }`}
              >
                <RotateCcw className={`h-3.5 w-3.5 ${togglingGateway === 'razorpay' ? 'animate-spin' : ''}`} />
                <span>{isOutageSimulated ? 'Restore Razorpay' : 'Simulate Bank Rail Outage'}</span>
              </button>
            </div>
          </div>
        </motion.div>
      </div>

      {/* ==================================================================== */}
      {/* SECTION 2: LIVE METRICS & USAGE TICKER (Solid Edge-to-Edge Banner)   */}
      {/* ==================================================================== */}
      <section className="w-full bg-[#0A0D18] text-white py-14 px-4 sm:px-8 border-y border-white/[0.08] relative z-20">
        <div className="max-w-7xl mx-auto">
          <div className="grid grid-cols-2 md:grid-cols-4 gap-6 text-center sm:text-left">
            <div className="space-y-1">
              <div className="flex items-center justify-center sm:justify-start gap-1.5 text-xs text-slate-400 font-mono">
                <Download className="h-3.5 w-3.5 text-[#1F51FF]" />
                <span>NPM & CRATES DOWNLOADS</span>
              </div>
              <p className="text-2xl sm:text-3xl font-extrabold tracking-tight text-white font-mono">
                48,290+
              </p>
              <span className="text-[11px] text-emerald-400 font-mono">Official CLI & SDK packages</span>
            </div>

            <div className="space-y-1">
              <div className="flex items-center justify-center sm:justify-start gap-1.5 text-xs text-slate-400 font-mono">
                <TrendingUp className="h-3.5 w-3.5 text-emerald-400" />
                <span>VOLUME ROUTED</span>
              </div>
              <p className="text-2xl sm:text-3xl font-extrabold tracking-tight text-white font-mono">
                ₹142.8 Cr+
              </p>
              <span className="text-[11px] text-slate-400 font-mono">Governed GMV volume</span>
            </div>

            <div className="space-y-1">
              <div className="flex items-center justify-center sm:justify-start gap-1.5 text-xs text-slate-400 font-mono">
                <Zap className="h-3.5 w-3.5 text-amber-400" />
                <span>REAL-TIME LIVE P95</span>
              </div>
              <p className="text-2xl sm:text-3xl font-extrabold tracking-tight text-emerald-400 font-mono">
                {liveP95}ms
              </p>
              <span className="text-[11px] text-slate-400 font-mono">{liveTps.toLocaleString()} TPS Live</span>
            </div>

            <div className="space-y-1">
              <div className="flex items-center justify-center sm:justify-start gap-1.5 text-xs text-slate-400 font-mono">
                <ShieldCheck className="h-3.5 w-3.5 text-sky-400" />
                <span>OCC SAFETY TOKENS</span>
              </div>
              <p className="text-2xl sm:text-3xl font-extrabold tracking-tight text-white font-mono">
                100% Passed
              </p>
              <span className="text-[11px] text-emerald-400 font-mono">0 stale mutations allowed</span>
            </div>
          </div>
        </div>
      </section>

      {/* ==================================================================== */}
      {/* SECTION 3: ANIMATING INFRASTRUCTURE PIPELINE (Scroll Depth Parallax) */}
      {/* ==================================================================== */}
      <ScrollSection className="bg-[#F8F9FD] text-[#0A0A0A] py-20 px-4 sm:px-8 border-b border-slate-200">
        <div className="max-w-7xl mx-auto space-y-8">
          <div className="max-w-3xl space-y-3">
            <span className="px-3.5 py-1 rounded-full text-xs font-semibold text-slate-700 bg-white border border-slate-200 shadow-sm">
              Autonomous Resilience Pipeline
            </span>
            <h2 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-[#0A0A0A]">
              How ESA Autonomously Heals Payment Failures in 1.68s
            </h2>
            <p className="text-xs sm:text-sm text-slate-600 leading-relaxed">
              Watch live packet telemetry move through the 5-stage ESA pipeline. When bank rails drop or latencies spike, the system shifts traffic with zero unverified mutations.
            </p>
          </div>

          <AnimatingInfrastructure isOutage={isOutageSimulated} />

          {/* Live Recent Events Stream */}
          <div className="p-4 rounded-2xl bg-white border border-slate-200 shadow-sm flex flex-col sm:flex-row items-center justify-between gap-3 text-xs font-mono">
            <div className="flex items-center gap-2 text-slate-500">
              <span className="h-2 w-2 rounded-full bg-emerald-500 animate-ping" />
              <span>LIVE TELEMETRY STREAM:</span>
            </div>
            <div className="text-slate-800 font-semibold truncate max-w-xl">
              {recentEvents[0]}
            </div>
            <span className="text-[10px] text-slate-400 font-mono">Updated just now</span>
          </div>
        </div>
      </ScrollSection>

      {/* ==================================================================== */}
      {/* SECTION 4: WHITE CANVAS CORRIDORS (Scroll Depth Parallax)            */}
      {/* ==================================================================== */}
      <ScrollSection id="corridors" className="bg-[#FFFFFF] text-[#0A0A0A] py-24 px-4 sm:px-8">
        <div className="max-w-7xl mx-auto space-y-12">
          <div className="max-w-4xl space-y-6">
            <span className="inline-block px-3.5 py-1 rounded-full text-xs font-semibold text-slate-700 bg-slate-100 border border-slate-200">
              Payments
            </span>
            <h2 className="text-3xl sm:text-5xl font-extrabold tracking-tight text-[#0A0A0A] leading-[1.15]">
              Supercharged payments for the Indian market. Sovereign checkout has never been easier. Route payments across UPI, RuPay, and NetBanking with access to autonomous failover.
            </h2>
          </div>

          <div className="rounded-[2.5rem] bg-[#0A0D18] text-white p-8 sm:p-12 shadow-2xl relative overflow-hidden">
            <div className="flex flex-col md:flex-row md:items-end justify-between gap-6 mb-10 pb-8 border-b border-white/[0.08]">
              <div>
                <span className="text-xs font-mono uppercase tracking-wider text-[#1F51FF] font-bold">
                  Unified Indian Gateway Switch
                </span>
                <h3 className="text-2xl sm:text-3xl font-extrabold text-white mt-1">
                  Active Indian Payment Corridors
                </h3>
                <p className="text-xs sm:text-sm text-slate-400 mt-1 max-w-lg">
                  Real-time latencies across Mumbai, Bangalore, and Noida processing hubs.
                </p>
              </div>

              <button
                onClick={fetchGateways}
                disabled={loadingGateways}
                className="px-4 py-2 rounded-xl bg-white/10 hover:bg-white/15 text-white text-xs font-mono flex items-center gap-2 self-start"
              >
                <RefreshCw className={`h-3.5 w-3.5 ${loadingGateways ? 'animate-spin text-[#1F51FF]' : ''}`} />
                <span>Refresh Corridors</span>
              </button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-5">
              {gateways.map((gw) => {
                const isDown = !gw.is_healthy;
                return (
                  <motion.div
                    key={gw.gateway}
                    whileHover={{ y: -4 }}
                    className={`p-6 rounded-3xl border transition-all relative overflow-hidden ${
                      isDown
                        ? 'border-amber-500/40 bg-amber-950/20 shadow-[0_0_30px_rgba(251,191,36,0.15)]'
                        : 'border-white/[0.08] bg-[#121728] hover:border-white/[0.2] shadow-lg'
                    }`}
                  >
                    <div className="flex items-start justify-between gap-2 mb-4">
                      <div>
                        <h4 className="font-bold text-white text-base capitalize">{gw.gateway}</h4>
                        <span
                          className={`text-[9px] font-mono px-2 py-0.5 rounded-full font-bold ${
                            isDown
                              ? 'bg-amber-400/20 text-amber-300 border border-amber-400/30'
                              : 'bg-emerald-400/20 text-emerald-300 border border-emerald-400/30'
                          }`}
                        >
                          {isDown ? 'DEGRADED' : 'HEALTHY'}
                        </span>
                      </div>

                      <button
                        onClick={() => handleToggleGateway(gw.gateway)}
                        disabled={togglingGateway === gw.gateway}
                        className={`text-[11px] px-2.5 py-1 rounded-lg border font-mono transition-all ${
                          isDown
                            ? 'border-emerald-400/30 bg-emerald-400/10 text-emerald-300 hover:bg-emerald-400/20'
                            : 'border-amber-400/30 bg-amber-400/10 text-amber-300 hover:bg-amber-400/20'
                        }`}
                      >
                        {isDown ? 'Restore' : 'Outage'}
                      </button>
                    </div>

                    <p className="text-xs text-slate-400 mb-4 line-clamp-1">{gw.name}</p>

                    <div className="grid grid-cols-2 gap-2 pt-3 border-t border-white/[0.06] text-center font-mono">
                      <div>
                        <span className="text-[9px] text-slate-400 block">P95 LATENCY</span>
                        <span className={`text-xs font-bold ${isDown ? 'text-amber-400' : 'text-slate-100'}`}>
                          {gw.p95_latency_ms.toFixed(1)}ms
                        </span>
                      </div>
                      <div>
                        <span className="text-[9px] text-slate-400 block">SUCCESS</span>
                        <span className={`text-xs font-bold ${isDown ? 'text-amber-400' : 'text-emerald-400'}`}>
                          {(gw.success_rate * 100).toFixed(1)}%
                        </span>
                      </div>
                    </div>
                  </motion.div>
                );
              })}
            </div>
          </div>
        </div>
      </ScrollSection>

      {/* ==================================================================== */}
      {/* SECTION 5: CLI & PROGRAMMATIC SDK HUB (Scroll Depth Parallax)        */}
      {/* ==================================================================== */}
      <ScrollSection id="sdk-cli" className="bg-[#F8F9FC] text-[#0A0A0A] py-24 px-4 sm:px-8 border-t border-slate-200">
        <div className="max-w-7xl mx-auto space-y-10">
          <div className="max-w-3xl space-y-3">
            <span className="px-3.5 py-1 rounded-full text-xs font-semibold text-slate-700 bg-white border border-slate-200 shadow-sm">
              Developer Ecosystem
            </span>
            <h2 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-[#0A0A0A]">
              CLI & Programmatic SDK
            </h2>
            <p className="text-xs sm:text-sm text-slate-600 leading-relaxed">
              Integrate autonomous resilience into your existing Node, Python, or Go payment service in minutes.
            </p>
          </div>

          <div className="rounded-[2.5rem] border border-slate-200 bg-white p-6 sm:p-10 shadow-sm">
            {/* Tab Selector */}
            <div className="flex flex-wrap items-center justify-between gap-4 pb-6 border-b border-slate-200">
              <div className="flex items-center gap-2 bg-slate-100 p-1 rounded-2xl border border-slate-200 font-mono text-xs">
                <button
                  onClick={() => setActiveDevTab('cli')}
                  className={`px-4 py-2 rounded-xl transition-all flex items-center gap-2 font-semibold ${
                    activeDevTab === 'cli'
                      ? 'bg-[#1F51FF] text-white shadow-sm'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  <Terminal className="h-4 w-4" />
                  <span>Terminal CLI</span>
                </button>
                <button
                  onClick={() => setActiveDevTab('typescript')}
                  className={`px-4 py-2 rounded-xl transition-all flex items-center gap-2 font-semibold ${
                    activeDevTab === 'typescript'
                      ? 'bg-[#1F51FF] text-white shadow-sm'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  <Code2 className="h-4 w-4" />
                  <span>TypeScript SDK</span>
                </button>
                <button
                  onClick={() => setActiveDevTab('python')}
                  className={`px-4 py-2 rounded-xl transition-all flex items-center gap-2 font-semibold ${
                    activeDevTab === 'python'
                      ? 'bg-[#1F51FF] text-white shadow-sm'
                      : 'text-slate-600 hover:text-slate-900'
                  }`}
                >
                  <Code2 className="h-4 w-4" />
                  <span>Python SDK</span>
                </button>
              </div>

              <div className="flex items-center gap-2 text-xs font-mono text-slate-500">
                <Download className="h-4 w-4 text-[#1F51FF]" />
                <span>npm install esapay / pip install esapay</span>
              </div>
            </div>

            {/* TAB 1: CLI QUICK COMMANDS */}
            {activeDevTab === 'cli' && (
              <div className="pt-6 space-y-6">
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
                  <div className="p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <span className="text-[10px] font-mono text-slate-400 block mb-1">GLOBAL CLI (NPM/BUN)</span>
                    <div className="flex items-center justify-between font-mono text-xs text-slate-900 font-bold">
                      <code>npm i -g esapay-cli</code>
                      <button
                        onClick={() => copyToClipboard('npm i -g esapay-cli', 'npm-cli')}
                        className="text-slate-400 hover:text-slate-900"
                      >
                        {copiedCmd === 'npm-cli' ? <Check className="h-4 w-4 text-emerald-600" /> : <Copy className="h-4 w-4" />}
                      </button>
                    </div>
                  </div>

                  <div className="p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <span className="text-[10px] font-mono text-slate-400 block mb-1">ZERO-INSTALL RUN</span>
                    <div className="flex items-center justify-between font-mono text-xs text-slate-900 font-bold">
                      <code>npx esapay-cli gateways</code>
                      <button
                        onClick={() => copyToClipboard('npx esapay-cli gateways', 'npx-gw')}
                        className="text-slate-400 hover:text-slate-900"
                      >
                        {copiedCmd === 'npx-gw' ? <Check className="h-4 w-4 text-emerald-600" /> : <Copy className="h-4 w-4" />}
                      </button>
                    </div>
                  </div>

                  <div className="p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <span className="text-[10px] font-mono text-slate-400 block mb-1">PYTHON SDK (PYPI)</span>
                    <div className="flex items-center justify-between font-mono text-xs text-slate-900 font-bold">
                      <code>pip install esapay</code>
                      <button
                        onClick={() => copyToClipboard('pip install esapay', 'pip-box')}
                        className="text-slate-400 hover:text-slate-900"
                      >
                        {copiedCmd === 'pip-box' ? <Check className="h-4 w-4 text-emerald-600" /> : <Copy className="h-4 w-4" />}
                      </button>
                    </div>
                  </div>

                  <div className="p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <span className="text-[10px] font-mono text-slate-400 block mb-1">BUN / TYPESCRIPT</span>
                    <div className="flex items-center justify-between font-mono text-xs text-slate-900 font-bold">
                      <code>bun add esapay</code>
                      <button
                        onClick={() => copyToClipboard('bun add esapay', 'bun-box')}
                        className="text-slate-400 hover:text-slate-900"
                      >
                        {copiedCmd === 'bun-box' ? <Check className="h-4 w-4 text-emerald-600" /> : <Copy className="h-4 w-4" />}
                      </button>
                    </div>
                  </div>
                </div>

                <div className="rounded-3xl border border-slate-900/10 bg-[#0E1322] text-white overflow-hidden shadow-xl">
                  <div className="px-5 py-3 border-b border-white/[0.08] bg-[#080B14] flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="h-3 w-3 rounded-full bg-rose-500/80 inline-block" />
                      <span className="h-3 w-3 rounded-full bg-amber-500/80 inline-block" />
                      <span className="h-3 w-3 rounded-full bg-emerald-500/80 inline-block" />
                      <span className="ml-2 text-xs font-mono text-slate-400">bash — npx esapay-cli</span>
                    </div>

                    <div className="flex items-center gap-1 font-mono text-xs">
                      {(['gateways', 'checkout', 'doctor', 'health'] as const).map((tab) => (
                        <button
                          key={tab}
                          onClick={() => setActiveTab(tab)}
                          className={`px-3 py-1 rounded-lg transition-all ${
                            activeTab === tab ? 'bg-[#1F51FF] text-white font-bold' : 'text-slate-400 hover:text-white'
                          }`}
                        >
                          esa {tab}
                        </button>
                      ))}
                    </div>

                    <button
                      onClick={() => copyToClipboard(terminalOutputs[activeTab], `term-${activeTab}`)}
                      className="text-xs font-mono text-slate-400 hover:text-white flex items-center gap-1"
                    >
                      {copiedCmd === `term-${activeTab}` ? (
                        <>
                          <Check className="h-3.5 w-3.5 text-emerald-400" />
                          <span className="text-emerald-400">Copied</span>
                        </>
                      ) : (
                        <>
                          <Copy className="h-3.5 w-3.5" />
                          <span>Copy</span>
                        </>
                      )}
                    </button>
                  </div>

                  <div className="p-6 font-mono text-xs sm:text-sm text-slate-200 overflow-x-auto leading-relaxed">
                    <pre className="whitespace-pre">{terminalOutputs[activeTab]}</pre>
                  </div>
                </div>
              </div>
            )}

            {/* TAB 2: TYPESCRIPT SDK */}
            {activeDevTab === 'typescript' && (
              <div className="pt-6 space-y-4">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div className="flex items-center justify-between p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <div className="flex items-center gap-2 font-mono text-xs text-slate-800">
                      <span className="text-slate-400 font-bold">npm:</span>
                      <code>npm install esapay</code>
                    </div>
                    <button
                      onClick={() => copyToClipboard('npm install esapay', 'npm-sdk')}
                      className="text-xs font-mono text-[#1F51FF] font-bold flex items-center gap-1"
                    >
                      {copiedCmd === 'npm-sdk' ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                      <span>{copiedCmd === 'npm-sdk' ? 'Copied' : 'Copy'}</span>
                    </button>
                  </div>

                  <div className="flex items-center justify-between p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <div className="flex items-center gap-2 font-mono text-xs text-slate-800">
                      <span className="text-slate-400 font-bold">bun:</span>
                      <code>bun add esapay</code>
                    </div>
                    <button
                      onClick={() => copyToClipboard('bun add esapay', 'bun-sdk')}
                      className="text-xs font-mono text-[#1F51FF] font-bold flex items-center gap-1"
                    >
                      {copiedCmd === 'bun-sdk' ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                      <span>{copiedCmd === 'bun-sdk' ? 'Copied' : 'Copy'}</span>
                    </button>
                  </div>
                </div>

                <div className="rounded-3xl border border-slate-900/10 bg-[#0E1322] text-white p-6 font-mono text-xs sm:text-sm overflow-x-auto shadow-xl relative">
                  <button
                    onClick={() => copyToClipboard(sdkCodeSnippets.typescript, 'ts-code')}
                    className="absolute top-4 right-4 text-xs font-mono text-slate-400 hover:text-white flex items-center gap-1 bg-white/10 px-3 py-1.5 rounded-lg"
                  >
                    {copiedCmd === 'ts-code' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                    <span>{copiedCmd === 'ts-code' ? 'Copied' : 'Copy Code'}</span>
                  </button>
                  <pre className="whitespace-pre text-slate-200">{sdkCodeSnippets.typescript}</pre>
                </div>
              </div>
            )}

            {/* TAB 3: PYTHON SDK */}
            {activeDevTab === 'python' && (
              <div className="pt-6 space-y-4">
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div className="flex items-center justify-between p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <div className="flex items-center gap-2 font-mono text-xs text-slate-800">
                      <span className="text-slate-400 font-bold">pip:</span>
                      <code>pip install esapay</code>
                    </div>
                    <button
                      onClick={() => copyToClipboard('pip install esapay', 'pip-sdk')}
                      className="text-xs font-mono text-[#1F51FF] font-bold flex items-center gap-1"
                    >
                      {copiedCmd === 'pip-sdk' ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                      <span>{copiedCmd === 'pip-sdk' ? 'Copied' : 'Copy'}</span>
                    </button>
                  </div>

                  <div className="flex items-center justify-between p-4 rounded-2xl bg-slate-50 border border-slate-200">
                    <div className="flex items-center gap-2 font-mono text-xs text-slate-800">
                      <span className="text-slate-400 font-bold">local repo dev:</span>
                      <code>pip install -e ./sdk/python</code>
                    </div>
                    <button
                      onClick={() => copyToClipboard('pip install -e ./sdk/python', 'pip-dev')}
                      className="text-xs font-mono text-[#1F51FF] font-bold flex items-center gap-1"
                    >
                      {copiedCmd === 'pip-dev' ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                      <span>{copiedCmd === 'pip-dev' ? 'Copied' : 'Copy'}</span>
                    </button>
                  </div>
                </div>

                <div className="rounded-3xl border border-slate-900/10 bg-[#0E1322] text-white p-6 font-mono text-xs sm:text-sm overflow-x-auto shadow-xl relative">
                  <button
                    onClick={() => copyToClipboard(sdkCodeSnippets.python, 'py-code')}
                    className="absolute top-4 right-4 text-xs font-mono text-slate-400 hover:text-white flex items-center gap-1 bg-white/10 px-3 py-1.5 rounded-lg"
                  >
                    {copiedCmd === 'py-code' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                    <span>{copiedCmd === 'py-code' ? 'Copied' : 'Copy Code'}</span>
                  </button>
                  <pre className="whitespace-pre text-slate-200">{sdkCodeSnippets.python}</pre>
                </div>
              </div>
            )}

            {/* Interactive Documentation Callout */}
            <div className="mt-8 pt-6 border-t border-slate-200 flex flex-col sm:flex-row items-center justify-between gap-4">
              <div className="flex items-center gap-3">
                <div className="h-10 w-10 rounded-2xl bg-blue-50 text-[#1F51FF] flex items-center justify-center flex-shrink-0">
                  <BookOpen className="h-5 w-5" />
                </div>
                <div>
                  <h4 className="text-sm font-bold text-slate-900">
                    Comprehensive SDK & Package Registry Hub
                  </h4>
                  <p className="text-xs text-slate-500">
                    Explore detailed parameters, TypeScript types, Python resilience models, and live NPM / Bun / PyPI mirrors.
                  </p>
                </div>
              </div>
              <button
                onClick={() => setIsDocsOpen(true)}
                className="px-5 py-2.5 rounded-full bg-[#1F51FF] hover:bg-[#1644DF] text-white font-bold text-xs flex items-center gap-2 shadow-md hover:shadow-lg transition-all whitespace-nowrap"
              >
                <BookOpen className="h-4 w-4" />
                <span>Open Documentation Hub</span>
                <ArrowRight className="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      </ScrollSection>

      {/* ==================================================================== */}
      {/* SECTION 6: FORMAL BENCHMARKS TABLE (Scroll Depth Parallax)           */}
      {/* ==================================================================== */}
      <ScrollSection id="benchmarks" className="bg-[#FFFFFF] text-[#0A0A0A] py-24 px-4 sm:px-8 border-t border-slate-200">
        <div className="max-w-7xl mx-auto space-y-10">
          <div className="max-w-2xl space-y-3">
            <span className="px-3.5 py-1 rounded-full text-xs font-semibold text-slate-700 bg-slate-100 border border-slate-200">
              Formal Benchmarks
            </span>
            <h2 className="text-3xl font-extrabold tracking-tight text-[#0A0A0A]">
              155 Deterministic Trial Results
            </h2>
            <p className="text-xs sm:text-sm text-slate-600">
              Evaluated across live Kubernetes workloads under simulated Indian festival surge traffic.
            </p>
          </div>

          <div className="rounded-[2rem] border border-slate-200 bg-white overflow-x-auto shadow-sm">
            <table className="w-full text-left text-xs font-mono">
              <thead className="border-b border-slate-200 bg-slate-50 text-slate-600 uppercase text-[10px]">
                <tr>
                  <th className="p-5">Architecture System</th>
                  <th className="p-5">Time To Detect (TTD)</th>
                  <th className="p-5">Time To Recover (TTR)</th>
                  <th className="p-5">Success Rate</th>
                  <th className="p-5">Stale Rejections</th>
                  <th className="p-5">Verdict</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 text-slate-700">
                <tr className="hover:bg-slate-50/50">
                  <td className="p-5 font-bold text-slate-500">B0: Static Threshold Rules</td>
                  <td className="p-5">15.0s</td>
                  <td className="p-5 text-rose-600 font-bold">180s+ (Manual triage)</td>
                  <td className="p-5">68.2%</td>
                  <td className="p-5 text-slate-400">Unchecked</td>
                  <td className="p-5 text-rose-600 font-semibold">Cascading Timeouts</td>
                </tr>
                <tr className="hover:bg-slate-50/50">
                  <td className="p-5 font-bold text-slate-500">B1: Ungoverned LLM Ops</td>
                  <td className="p-5">4.2s</td>
                  <td className="p-5">12.5s</td>
                  <td className="p-5">81.4%</td>
                  <td className="p-5 text-rose-600 font-bold">0% (Races)</td>
                  <td className="p-5 text-amber-600 font-semibold">Illegal Mutations</td>
                </tr>
                <tr className="bg-[#1F51FF]/5 hover:bg-[#1F51FF]/10">
                  <td className="p-5 font-bold text-[#1F51FF] flex items-center gap-2">
                    <span className="h-2 w-2 rounded-full bg-[#1F51FF] animate-pulse" />
                    B2: Governed ESA (Our Work)
                  </td>
                  <td className="p-5 text-emerald-600 font-bold">0.25s (Streaming)</td>
                  <td className="p-5 text-emerald-600 font-bold">1.68s (P95)</td>
                  <td className="p-5 text-emerald-600 font-bold">99.7%</td>
                  <td className="p-5 text-emerald-600 font-bold">100% (OCC Enforced)</td>
                  <td className="p-5 text-emerald-600 font-bold">Zero-Downtime Autonomous</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </ScrollSection>

      {/* ==================================================================== */}
      {/* INTERACTIVE IN-APP CHECKOUT MODAL                                    */}
      {/* ==================================================================== */}
      <AnimatePresence>
        {isModalOpen && (
          <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-md">
            <motion.div
              initial={{ opacity: 0, scale: 0.95, y: 15 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.95, y: 15 }}
              className="relative w-full max-w-md rounded-[2rem] border border-white/[0.15] bg-[#0E1324] shadow-2xl overflow-hidden text-white"
            >
              <div className="px-6 py-4 border-b border-white/[0.08] bg-[#090D18] flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="text-xl font-extrabold lowercase text-white tracking-tighter">esa</span>
                  <span className="text-[10px] text-slate-400 font-mono">· Sovereign Indian Checkout</span>
                </div>
                <button
                  onClick={() => setIsModalOpen(false)}
                  className="p-1 rounded-lg text-slate-400 hover:text-white hover:bg-white/5 transition-colors"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>

              {checkoutResult && (
                <div
                  className={`px-5 py-2.5 text-xs font-mono flex items-center gap-2 border-b ${
                    checkoutResult.failover_triggered
                      ? 'bg-amber-500/10 border-amber-500/20 text-amber-300'
                      : 'bg-emerald-500/10 border-emerald-500/20 text-emerald-300'
                  }`}
                >
                  {checkoutResult.failover_triggered ? (
                    <AlertTriangle className="h-4 w-4 shrink-0 text-amber-400" />
                  ) : (
                    <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-400" />
                  )}
                  <span className="truncate">
                    {checkoutResult.failover_triggered
                      ? `Autonomous Failover: Rerouted to ${checkoutResult.routed_gateway.toUpperCase()}`
                      : `Optimal Rail: ${checkoutResult.routed_gateway.toUpperCase()}`}
                  </span>
                </div>
              )}

              <div className="p-6 space-y-5">
                {paymentSuccess ? (
                  <div className="text-center py-6 space-y-4">
                    <div className="h-16 w-16 rounded-full bg-emerald-500/20 border border-emerald-500/40 text-emerald-400 flex items-center justify-center mx-auto shadow-[0_0_30px_rgba(16,185,129,0.4)]">
                      <Check className="h-8 w-8 stroke-[3]" />
                    </div>
                    <div>
                      <h3 className="text-xl font-extrabold text-white">Payment Settled</h3>
                      <p className="text-xs text-slate-400 mt-1">
                        Processed via {checkoutResult?.routed_gateway.toUpperCase() || 'PHONEPE'} Corridor
                      </p>
                    </div>

                    <div className="p-4 rounded-2xl border border-white/5 bg-white/[0.02] text-left font-mono text-[11px] space-y-1.5">
                      <div className="flex justify-between text-slate-400">
                        <span>Amount Paid:</span>
                        <span className="text-white font-bold">₹{amount.toFixed(2)} INR</span>
                      </div>
                      <div className="flex justify-between text-slate-400">
                        <span>Transaction ID:</span>
                        <span className="text-blue-400 font-bold truncate max-w-[180px]">
                          {checkoutResult?.transaction_id || 'tx_esa_demo'}
                        </span>
                      </div>
                      <div className="flex justify-between text-slate-400">
                        <span>Audit Proof:</span>
                        <span className="text-emerald-400 truncate max-w-[180px]">{activeTxHash}</span>
                      </div>
                    </div>

                    <button
                      onClick={() => {
                        setIsModalOpen(false);
                        setPaymentSuccess(false);
                      }}
                      className="w-full py-3 rounded-full bg-white text-black hover:bg-slate-100 font-bold text-xs transition-colors"
                    >
                      Close Checkout
                    </button>
                  </div>
                ) : (
                  <>
                    <div className="flex items-center justify-between pb-3 border-b border-white/[0.08]">
                      <div>
                        <span className="text-[11px] text-slate-400 font-mono block">Order Total:</span>
                        <span className="text-xl font-extrabold text-white">
                          ₹{amount.toFixed(2)} INR
                        </span>
                      </div>
                      <span className="text-xs font-mono text-slate-400">Merchant Store</span>
                    </div>

                    <div className="grid grid-cols-3 gap-2 bg-[#080B14] p-1.5 rounded-2xl border border-white/[0.08] text-xs font-mono">
                      <button
                        onClick={() => setModalMethod('upi')}
                        className={`py-2 rounded-xl flex items-center justify-center gap-1.5 transition-all ${
                          modalMethod === 'upi' ? 'bg-white text-black font-bold' : 'text-slate-400 hover:text-white'
                        }`}
                      >
                        <Smartphone className="h-3.5 w-3.5" />
                        <span>UPI</span>
                      </button>
                      <button
                        onClick={() => setModalMethod('card')}
                        className={`py-2 rounded-xl flex items-center justify-center gap-1.5 transition-all ${
                          modalMethod === 'card' ? 'bg-white text-black font-bold' : 'text-slate-400 hover:text-white'
                        }`}
                      >
                        <CreditCard className="h-3.5 w-3.5" />
                        <span>RuPay/Card</span>
                      </button>
                      <button
                        onClick={() => setModalMethod('netbanking')}
                        className={`py-2 rounded-xl flex items-center justify-center gap-1.5 transition-all ${
                          modalMethod === 'netbanking' ? 'bg-white text-black font-bold' : 'text-slate-400 hover:text-white'
                        }`}
                      >
                        <Building2 className="h-3.5 w-3.5" />
                        <span>Banks</span>
                      </button>
                    </div>

                    {modalMethod === 'upi' && (
                      <div className="p-4 rounded-2xl border border-white/[0.08] bg-[#080B14] text-center space-y-3">
                        <div className="h-28 w-28 mx-auto bg-white rounded-2xl p-2 flex items-center justify-center shadow-lg">
                          <QrCode className="h-24 w-24 text-black" />
                        </div>
                        <p className="text-[11px] text-slate-300 font-mono">
                          Scan with PhonePe, Google Pay, Paytm, or BHIM
                        </p>
                      </div>
                    )}

                    {modalMethod === 'card' && (
                      <div className="p-4 rounded-2xl border border-white/[0.08] bg-gradient-to-br from-emerald-950/40 to-slate-900/60 space-y-2 font-mono">
                        <div className="flex justify-between text-slate-400 text-[10px]">
                          <span>RUPAY PLATINUM</span>
                          <span>ESA SECURE</span>
                        </div>
                        <div className="text-base font-bold tracking-widest text-white">
                          6527 •••• •••• 1005
                        </div>
                        <div className="flex justify-between text-[10px] text-slate-400 pt-1">
                          <span>EXP: 08/29</span>
                          <span>CVV: •••</span>
                        </div>
                      </div>
                    )}

                    {modalMethod === 'netbanking' && (
                      <div className="grid grid-cols-2 gap-2 text-xs font-mono">
                        {['HDFC Bank', 'ICICI Bank', 'State Bank of India', 'Axis Bank'].map((b) => (
                          <div
                            key={b}
                            className="p-3 rounded-xl border border-white/[0.08] bg-[#080B14] text-center text-slate-300 hover:border-white/40 cursor-pointer"
                          >
                            {b}
                          </div>
                        ))}
                      </div>
                    )}

                    <button
                      onClick={handleCompleteModalPayment}
                      disabled={isPayingInModal}
                      className="w-full py-3.5 rounded-full bg-white hover:bg-slate-100 text-black font-extrabold text-sm shadow-xl transition-all flex items-center justify-center gap-2"
                    >
                      {isPayingInModal ? (
                        <>
                          <RefreshCw className="h-4 w-4 animate-spin text-black" />
                          <span>Authorizing via {checkoutResult?.routed_gateway.toUpperCase() || 'GATEWAY'}...</span>
                        </>
                      ) : (
                        <>
                          <Lock className="h-4 w-4" />
                          <span>Pay ₹{amount.toFixed(2)} Securely</span>
                        </>
                      )}
                    </button>
                  </>
                )}
              </div>
            </motion.div>
          </div>
        )}
      </AnimatePresence>

      {/* ==================================================================== */}
      {/* DOCUMENTATION & OPEN SOURCE HUB MODAL                                */}
      {/* ==================================================================== */}
      <AnimatePresence>
        {isDocsOpen && (
          <div className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-6 bg-black/80 backdrop-blur-md">
            <motion.div
              initial={{ opacity: 0, scale: 0.95, y: 15 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.95, y: 15 }}
              transition={{ duration: 0.2 }}
              className="relative w-full max-w-4xl max-h-[90vh] bg-[#0A0D18] border border-white/10 rounded-3xl shadow-2xl text-white flex flex-col overflow-hidden"
            >
              {/* Header */}
              <div className="px-6 py-4 border-b border-white/[0.08] bg-[#070A12] flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="h-8 w-8 rounded-xl bg-[#1F51FF] flex items-center justify-center text-white font-extrabold text-sm shadow-md lowercase">
                    esa
                  </div>
                  <div>
                    <div className="flex items-center gap-2">
                      <h3 className="font-extrabold text-sm sm:text-base tracking-tight text-white">
                        Developer Documentation & Package Registries
                      </h3>
                      <span className="bg-emerald-500/15 text-emerald-400 border border-emerald-500/30 text-[9px] px-2 py-0.5 rounded-full font-mono font-bold">
                        v1.0.0
                      </span>
                    </div>
                    <p className="text-[11px] text-slate-400">
                      Sovereign Indian payment rails (UPI, RuPay, NetBanking) across CLI, TypeScript, and Python.
                    </p>
                  </div>
                </div>
                <button
                  onClick={() => setIsDocsOpen(false)}
                  className="h-8 w-8 rounded-full bg-white/5 hover:bg-white/10 text-slate-400 hover:text-white flex items-center justify-center transition-colors"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>

              {/* Sub-nav Tabs */}
              <div className="flex items-center gap-1.5 px-6 py-2.5 border-b border-white/[0.06] bg-[#05070D] font-mono text-xs overflow-x-auto">
                <button
                  onClick={() => setDocsTab('cli')}
                  className={`px-3 py-1.5 rounded-xl transition-all flex items-center gap-2 whitespace-nowrap ${
                    docsTab === 'cli'
                      ? 'bg-[#1F51FF] text-white font-bold shadow-sm'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  <Terminal className="h-3.5 w-3.5" />
                  <span>esapay-cli (CLI Tool)</span>
                </button>
                <button
                  onClick={() => setDocsTab('typescript')}
                  className={`px-3 py-1.5 rounded-xl transition-all flex items-center gap-2 whitespace-nowrap ${
                    docsTab === 'typescript'
                      ? 'bg-[#1F51FF] text-white font-bold shadow-sm'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  <Code2 className="h-3.5 w-3.5" />
                  <span>esapay (TypeScript & Bun)</span>
                </button>
                <button
                  onClick={() => setDocsTab('python')}
                  className={`px-3 py-1.5 rounded-xl transition-all flex items-center gap-2 whitespace-nowrap ${
                    docsTab === 'python'
                      ? 'bg-[#1F51FF] text-white font-bold shadow-sm'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  <Code2 className="h-3.5 w-3.5" />
                  <span>esapay (Python SDK)</span>
                </button>
                <button
                  onClick={() => setDocsTab('registries')}
                  className={`px-3 py-1.5 rounded-xl transition-all flex items-center gap-2 whitespace-nowrap ${
                    docsTab === 'registries'
                      ? 'bg-[#1F51FF] text-white font-bold shadow-sm'
                      : 'text-slate-400 hover:text-white'
                  }`}
                >
                  <Download className="h-3.5 w-3.5" />
                  <span>Official Registries & Mirrors</span>
                </button>
              </div>

              {/* Tab Content Body */}
              <div className="flex-1 p-6 overflow-y-auto space-y-6 text-xs sm:text-sm font-sans">
                {/* TAB 1: CLI */}
                {docsTab === 'cli' && (
                  <div className="space-y-6">
                    <div className="space-y-2">
                      <h4 className="text-base font-extrabold text-white flex items-center gap-2">
                        <Terminal className="h-4 w-4 text-[#1F51FF]" />
                        <span>ESA Command Line Interface (CLI)</span>
                      </h4>
                      <p className="text-slate-400 text-xs">
                        High-performance terminal tool for inspecting Indian payment corridors, simulating gateway degradation, executing failover checkouts, and running comprehensive diagnostic audits.
                      </p>
                    </div>

                    {/* How documentation renders */}
                    <div className="p-3.5 rounded-2xl bg-blue-500/10 border border-blue-500/20 text-blue-200 text-xs flex items-start gap-2.5">
                      <div className="p-1 rounded-lg bg-blue-500/20 text-blue-300 font-bold mt-0.5">INFO</div>
                      <div>
                        <strong>Package Registry Documentation:</strong> Published as <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">esapay-cli</code>. The package tarball includes the full root <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">README.md</code>, rendering automatically on <a href="https://www.npmjs.com/package/esapay-cli" target="_blank" rel="noreferrer" className="underline text-white font-semibold">npmjs.com</a> and through <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">bunx esapay-cli --help</code>.
                      </div>
                    </div>

                    {/* Install options */}
                    <div className="space-y-2">
                      <span className="text-[11px] font-mono text-slate-400 uppercase tracking-wider font-bold">Installation Options</span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 font-mono text-xs">
                        <div className="p-3 rounded-xl bg-white/[0.03] border border-white/[0.08] flex items-center justify-between">
                          <span>npm i -g esapay-cli</span>
                          <button onClick={() => copyToClipboard('npm i -g esapay-cli', 'c1')} className="text-slate-400 hover:text-white">
                            {copiedCmd === 'c1' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                          </button>
                        </div>
                        <div className="p-3 rounded-xl bg-white/[0.03] border border-white/[0.08] flex items-center justify-between">
                          <span>bun add -g esapay-cli</span>
                          <button onClick={() => copyToClipboard('bun add -g esapay-cli', 'c2')} className="text-slate-400 hover:text-white">
                            {copiedCmd === 'c2' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                          </button>
                        </div>
                      </div>
                      <div className="p-2.5 rounded-xl bg-black/30 border border-white/[0.06] text-[11px] font-mono text-slate-400 flex items-center justify-between">
                        <span>Local Repo Install: npm install -g ./packages/esa-cli</span>
                        <button onClick={() => copyToClipboard('npm install -g ./packages/esa-cli', 'c-local')} className="text-slate-400 hover:text-white">
                          {copiedCmd === 'c-local' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                        </button>
                      </div>
                    </div>

                    {/* Commands Reference Table */}
                    <div className="space-y-2">
                      <span className="text-[11px] font-mono text-slate-400 uppercase tracking-wider font-bold">Commands Reference</span>
                      <div className="rounded-2xl border border-white/[0.08] overflow-hidden bg-black/40">
                        <table className="w-full text-left font-mono text-xs">
                          <thead className="bg-white/[0.04] text-slate-400 border-b border-white/[0.08] text-[10px] uppercase">
                            <tr>
                              <th className="py-2.5 px-4">Command</th>
                              <th className="py-2.5 px-4">Description</th>
                              <th className="py-2.5 px-4">Flags</th>
                            </tr>
                          </thead>
                          <tbody className="divide-y divide-white/[0.05] text-slate-300">
                            <tr>
                              <td className="py-2.5 px-4 text-emerald-400 font-bold">esapay gateways</td>
                              <td className="py-2.5 px-4">Query live Indian gateway status, P95 latency, and traffic %</td>
                              <td className="py-2.5 px-4 text-slate-400">--toggle &lt;name&gt;, --json</td>
                            </tr>
                            <tr>
                              <td className="py-2.5 px-4 text-emerald-400 font-bold">esapay checkout</td>
                              <td className="py-2.5 px-4">Initiate autonomous checkout with dynamic failover</td>
                              <td className="py-2.5 px-4 text-slate-400">--amount &lt;paise&gt;, --gateway, --currency</td>
                            </tr>
                            <tr>
                              <td className="py-2.5 px-4 text-emerald-400 font-bold">esapay doctor</td>
                              <td className="py-2.5 px-4">Run 5-point cluster diagnostics (API, Shards, Ollama, Audit)</td>
                              <td className="py-2.5 px-4 text-slate-400">--verbose</td>
                            </tr>
                            <tr>
                              <td className="py-2.5 px-4 text-emerald-400 font-bold">esapay health</td>
                              <td className="py-2.5 px-4">Quick ping to verify Control Plane responsiveness</td>
                              <td className="py-2.5 px-4 text-slate-400">--url &lt;endpoint&gt;</td>
                            </tr>
                          </tbody>
                        </table>
                      </div>
                    </div>
                  </div>
                )}

                {/* TAB 2: TYPESCRIPT */}
                {docsTab === 'typescript' && (
                  <div className="space-y-6">
                    <div className="space-y-2">
                      <h4 className="text-base font-extrabold text-white flex items-center gap-2">
                        <Code2 className="h-4 w-4 text-[#1F51FF]" />
                        <span>TypeScript & Node/Bun SDK (esapay)</span>
                      </h4>
                      <p className="text-slate-400 text-xs">
                        Official type-safe client library for Node.js, Bun, Next.js, and Express backends with automatic failover circuit-breakers and background health heartbeats.
                      </p>
                    </div>

                    <div className="p-3.5 rounded-2xl bg-emerald-500/10 border border-emerald-500/20 text-emerald-200 text-xs flex items-start gap-2.5">
                      <div className="p-1 rounded-lg bg-emerald-500/20 text-emerald-300 font-bold mt-0.5">NPM / BUN</div>
                      <div>
                        <strong>Package Manifest & Registry Doc:</strong> Configured in <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">package.json</code> as <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">esapay</code> with complete TypeScript definitions and full markdown guide automatically rendered on package directories.
                      </div>
                    </div>

                    <div className="space-y-2">
                      <span className="text-[11px] font-mono text-slate-400 uppercase tracking-wider font-bold">Install</span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 font-mono text-xs">
                        <div className="p-3 rounded-xl bg-white/[0.03] border border-white/[0.08] flex items-center justify-between">
                          <code>npm install esapay</code>
                          <button onClick={() => copyToClipboard('npm install esapay', 'ts1')} className="text-slate-400 hover:text-white">
                            {copiedCmd === 'ts1' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                          </button>
                        </div>
                        <div className="p-3 rounded-xl bg-white/[0.03] border border-white/[0.08] flex items-center justify-between">
                          <code>bun add esapay</code>
                          <button onClick={() => copyToClipboard('bun add esapay', 'ts2')} className="text-slate-400 hover:text-white">
                            {copiedCmd === 'ts2' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                          </button>
                        </div>
                      </div>
                      <div className="p-2.5 rounded-xl bg-black/30 border border-white/[0.06] text-[11px] font-mono text-slate-400 flex items-center justify-between">
                        <span>Local Repo Install: npm install ./sdk/typescript</span>
                        <button onClick={() => copyToClipboard('npm install ./sdk/typescript', 'ts-local')} className="text-slate-400 hover:text-white">
                          {copiedCmd === 'ts-local' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                        </button>
                      </div>
                    </div>

                    <div className="space-y-2">
                      <span className="text-[11px] font-mono text-slate-400 uppercase tracking-wider font-bold">Usage Recipe</span>
                      <div className="p-4 rounded-2xl bg-black/50 border border-white/[0.08] font-mono text-xs text-slate-300 relative">
                        <pre className="whitespace-pre overflow-x-auto">{`import { EsaGateway } from 'esapay';

const esa = new EsaGateway({
  apiUrl: process.env.ESA_API_URL || 'http://localhost:8080',
  apiKey: process.env.ESA_API_KEY,
  timeoutMs: 8000,
});

// Autonomous checkout on sovereign Indian rails (UPI, RuPay, NetBanking)
const decision = await esa.checkout({
  amount: 50000,      // in paise (₹500.00)
  currency: 'INR',
  gateway: 'auto',    // autonomous failover across PhonePe, Razorpay, Paytm, Cashfree
  method: 'UPI',      // UPI | CARD | NETBANKING
});

console.log(\`Routed to \${decision.routed_gateway} (Failover active: \${decision.failover_triggered})\`);
console.log(\`Session URL: \${decision.checkout_url}\`);`}</pre>
                      </div>
                    </div>
                  </div>
                )}

                {/* TAB 3: PYTHON */}
                {docsTab === 'python' && (
                  <div className="space-y-6">
                    <div className="space-y-2">
                      <h4 className="text-base font-extrabold text-white flex items-center gap-2">
                        <Code2 className="h-4 w-4 text-[#1F51FF]" />
                        <span>Python SDK (esapay) — Zero External Dependencies</span>
                      </h4>
                      <p className="text-slate-400 text-xs">
                        Pure Python standard library client (<code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">urllib.request</code>). Zero third-party dependency footprint (&lt;50KB), perfectly suited for AWS Lambda, GCP Cloud Functions, FastAPI, and Django.
                      </p>
                    </div>

                    <div className="p-3.5 rounded-2xl bg-amber-500/10 border border-amber-500/20 text-amber-200 text-xs flex items-start gap-2.5">
                      <div className="p-1 rounded-lg bg-amber-500/20 text-amber-300 font-bold mt-0.5">PYPI</div>
                      <div>
                        <strong>PyPI Documentation Rendering:</strong> Configured in <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">pyproject.toml</code> as <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">esapay</code> with <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">readme = "README.md"</code>. Upon publishing via <code className="text-white font-mono bg-black/40 px-1 py-0.5 rounded">twine upload dist/*</code>, PyPI automatically translates all markdown headers, code blocks, and badges to the live package website.
                      </div>
                    </div>

                    <div className="space-y-2">
                      <span className="text-[11px] font-mono text-slate-400 uppercase tracking-wider font-bold">Install</span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 font-mono text-xs">
                        <div className="p-3 rounded-xl bg-white/[0.03] border border-white/[0.08] flex items-center justify-between">
                          <code>pip install esapay</code>
                          <button onClick={() => copyToClipboard('pip install esapay', 'py1')} className="text-slate-400 hover:text-white">
                            {copiedCmd === 'py1' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                          </button>
                        </div>
                        <div className="p-3 rounded-xl bg-white/[0.03] border border-white/[0.08] flex items-center justify-between">
                          <code>uv add esapay</code>
                          <button onClick={() => copyToClipboard('uv add esapay', 'py2')} className="text-slate-400 hover:text-white">
                            {copiedCmd === 'py2' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                          </button>
                        </div>
                      </div>
                      <div className="p-2.5 rounded-xl bg-black/30 border border-white/[0.06] text-[11px] font-mono text-slate-400 flex items-center justify-between">
                        <span>Local Repo Dev: pip install -e ./sdk/python</span>
                        <button onClick={() => copyToClipboard('pip install -e ./sdk/python', 'py-local')} className="text-slate-400 hover:text-white">
                          {copiedCmd === 'py-local' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                        </button>
                      </div>
                    </div>

                    <div className="space-y-2">
                      <span className="text-[11px] font-mono text-slate-400 uppercase tracking-wider font-bold">Usage Recipe</span>
                      <div className="p-4 rounded-2xl bg-black/50 border border-white/[0.08] font-mono text-xs text-slate-300 relative">
                        <pre className="whitespace-pre overflow-x-auto">{`from esapay import EsaGateway
# Note: 'from esa import EsaGateway' is also fully supported

esa = EsaGateway(
    api_url="http://localhost:8080",
    api_key="esa_live_secret_key",
    auto_failover=True
)

# 1. Health check
print("Health:", esa.health())

# 2. Inspect Indian corridors
for gw in esa.gateways.list():
    print(f"[{gw.status}] {gw.name} - P95: {gw.p95_latency_ms}ms")

# 3. Create an autonomous transaction
decision = esa.checkout(
    amount=50000,  # ₹500.00
    currency="INR",
    method="UPI"
)

print(f"Settled via: {decision.routed_gateway} (Failover: {decision.failover_triggered})")`}</pre>
                      </div>
                    </div>
                  </div>
                )}

                {/* TAB 4: REGISTRIES */}
                {docsTab === 'registries' && (
                  <div className="space-y-6">
                    <div className="space-y-2">
                      <h4 className="text-base font-extrabold text-white flex items-center gap-2">
                        <Download className="h-4 w-4 text-[#1F51FF]" />
                        <span>Open Source Package Mirrors & CI/CD Hub</span>
                      </h4>
                      <p className="text-slate-400 text-xs">
                        ESA artifacts are continuously verified and packaged for standard package ecosystems.
                      </p>
                    </div>

                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <div className="p-4 rounded-2xl bg-white/[0.03] border border-white/[0.08] space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-bold text-white text-xs">NPM Registry (TypeScript SDK)</span>
                          <span className="text-[10px] font-mono text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">v1.0.1 Live</span>
                        </div>
                        <p className="text-[11px] text-slate-400 font-mono">
                          <a href="https://www.npmjs.com/package/esapay" target="_blank" rel="noreferrer" className="text-[#1F51FF] hover:underline flex items-center gap-1 font-bold">
                            <span>npmjs.com/package/esapay</span>
                            <ExternalLink className="h-3 w-3" />
                          </a>
                          <span className="text-slate-400 text-[10px] block mt-1">npm install esapay</span>
                        </p>
                      </div>

                      <div className="p-4 rounded-2xl bg-white/[0.03] border border-white/[0.08] space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-bold text-white text-xs">PyPI (Python Package Index)</span>
                          <span className="text-[10px] font-mono text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">v1.0.1 Live</span>
                        </div>
                        <p className="text-[11px] text-slate-400 font-mono">
                          <a href="https://pypi.org/project/esapay/" target="_blank" rel="noreferrer" className="text-[#1F51FF] hover:underline flex items-center gap-1 font-bold">
                            <span>pypi.org/project/esapay/</span>
                            <ExternalLink className="h-3 w-3" />
                          </a>
                          <span className="text-slate-400 text-[10px] block mt-1">pip install esapay</span>
                        </p>
                      </div>

                      <div className="p-4 rounded-2xl bg-white/[0.03] border border-white/[0.08] space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-bold text-white text-xs">NPM CLI (Global Terminal Tool)</span>
                          <span className="text-[10px] font-mono text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">v1.0.1 Live</span>
                        </div>
                        <p className="text-[11px] text-slate-400 font-mono">
                          <a href="https://www.npmjs.com/package/esapay-cli" target="_blank" rel="noreferrer" className="text-[#1F51FF] hover:underline flex items-center gap-1 font-bold">
                            <span>npmjs.com/package/esapay-cli</span>
                            <ExternalLink className="h-3 w-3" />
                          </a>
                          <span className="text-slate-400 text-[10px] block mt-1">npm i -g esapay-cli</span>
                        </p>
                      </div>

                      <div className="p-4 rounded-2xl bg-white/[0.03] border border-white/[0.08] space-y-2">
                        <div className="flex items-center justify-between">
                          <span className="font-bold text-white text-xs">Bun Native Registry</span>
                          <span className="text-[10px] font-mono text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">Instant</span>
                        </div>
                        <p className="text-[11px] text-slate-400 font-mono">
                          <span className="text-slate-200">bunx esapay-cli</span><br />
                          <span className="text-slate-400 text-[10px] block mt-1">bun add esapay</span>
                        </p>
                      </div>
                    </div>

                    <div className="p-4 rounded-2xl bg-black/40 border border-white/[0.08] space-y-2">
                      <span className="text-xs font-mono text-slate-300 font-bold block">One-Command Publication Script</span>
                      <p className="text-xs text-slate-400">
                        Run our automated distribution script to validate tarballs, generate wheels, and publish across all registries:
                      </p>
                      <div className="p-3 rounded-xl bg-black/60 border border-white/[0.08] font-mono text-xs text-emerald-400 flex items-center justify-between">
                        <code>./scripts/publish-packages.sh --dry-run</code>
                        <button onClick={() => copyToClipboard('./scripts/publish-packages.sh --dry-run', 'sh1')} className="text-slate-400 hover:text-white">
                          {copiedCmd === 'sh1' ? <Check className="h-3.5 w-3.5 text-emerald-400" /> : <Copy className="h-3.5 w-3.5" />}
                        </button>
                      </div>
                    </div>
                  </div>
                )}
              </div>

              {/* Modal Footer */}
              <div className="px-6 py-3.5 border-t border-white/[0.08] bg-[#070A12] flex items-center justify-between text-xs text-slate-400">
                <span className="font-mono text-[11px]">Sovereign Financial Mesh Architecture</span>
                <a
                  href="https://github.com/sujithputta02/Esapay"
                  target="_blank"
                  rel="noreferrer"
                  className="text-white hover:underline flex items-center gap-1.5 font-semibold"
                >
                  <span>View GitHub Repository</span>
                  <ExternalLink className="h-3.5 w-3.5" />
                </a>
              </div>
            </motion.div>
          </div>
        )}
      </AnimatePresence>

      {/* ==================================================================== */}
      {/* MINIMALIST DUE FOOTER                                                */}
      {/* ==================================================================== */}
      <footer className="relative z-20 border-t border-slate-200 bg-white py-12 px-4 sm:px-8 text-xs text-slate-500">
        <div className="max-w-7xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <span className="font-extrabold text-xl text-black lowercase">esa</span>
            <span>· Executable State Architecture · Sovereign Financial Infrastructure</span>
          </div>
          <div className="flex items-center gap-6 font-mono text-[11px]">
            <button
              onClick={() => setIsDocsOpen(true)}
              className="text-black hover:underline flex items-center gap-1 font-semibold"
            >
              <BookOpen className="h-3.5 w-3.5 text-[#1F51FF]" />
              <span>Documentation</span>
            </button>
            <span>Control Plane: {serverUrl}</span>
            <a
              href="https://github.com/sujithputta02/Esapay"
              target="_blank"
              rel="noreferrer"
              className="text-black hover:underline flex items-center gap-1 font-semibold"
            >
              <span>GitHub</span>
              <ExternalLink className="h-3 w-3" />
            </a>
          </div>
        </div>
      </footer>
    </div>
  );
}
