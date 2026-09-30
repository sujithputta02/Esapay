import { useLocation, Link } from 'react-router-dom';
import { motion } from 'framer-motion';
import {
  ShieldCheck,
  Zap,
  Server,
  Activity,
  ArrowLeft,
  CheckCircle2,
  Lock,
} from 'lucide-react';
import { EnterpriseAuthDialog } from '@/components/EnterpriseAuthDialog';

export function AuthPage() {
  const location = useLocation();
  const searchParams = new URLSearchParams(location.search);
  const isSignup =
    location.pathname.includes('/signup') ||
    searchParams.get('mode') === 'signup' ||
    searchParams.get('auth') === 'signup';

  return (
    <div className="min-h-screen bg-[#060913] text-white flex flex-col justify-between selection:bg-[#1F51FF] selection:text-white relative overflow-hidden font-sans">
      {/* Background ambient lighting */}
      <div className="pointer-events-none absolute top-0 left-1/4 w-[600px] h-[400px] bg-[#1F51FF]/15 blur-[140px] rounded-full" />
      <div className="pointer-events-none absolute bottom-0 right-1/4 w-[500px] h-[350px] bg-emerald-500/10 blur-[130px] rounded-full" />

      {/* Top Navbar */}
      <header className="relative z-20 px-6 sm:px-12 py-6 flex items-center justify-between border-b border-white/[0.06] backdrop-blur-md bg-[#060913]/60">
        <Link to="/" className="flex items-center gap-2 group">
          <ArrowLeft className="h-4 w-4 text-slate-400 group-hover:text-white group-hover:-translate-x-0.5 transition-all" />
          <span className="text-2xl font-extrabold tracking-tighter text-white lowercase">esa</span>
          <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-white/10 text-slate-300">
            ENTERPRISE PORTAL
          </span>
        </Link>

        <div className="flex items-center gap-3 text-xs font-mono text-slate-400">
          <span className="hidden sm:flex items-center gap-1.5 text-emerald-400">
            <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
            Cluster: ap-south-1 (Mumbai)
          </span>
          <span className="text-white/20 hidden sm:inline">|</span>
          <Link
            to="/dashboard?mode=sandbox"
            className="px-3 py-1.5 rounded-full bg-amber-400/10 hover:bg-amber-400/20 text-amber-300 border border-amber-400/30 transition-all flex items-center gap-1.5"
          >
            <Zap className="h-3 w-3 text-amber-400" />
            <span>Developer Sandbox</span>
          </Link>
        </div>
      </header>

      {/* Main Content: Split Enterprise Layout */}
      <main className="relative z-10 flex-1 max-w-7xl mx-auto w-full px-4 sm:px-8 py-8 sm:py-12 flex flex-col lg:flex-row items-center justify-center gap-10 lg:gap-16">
        {/* Left Column: Platform Security Assurances & Live Metrics */}
        <motion.div
          initial={{ opacity: 0, x: -20 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.5 }}
          className="w-full lg:w-1/2 space-y-6 lg:pr-6"
        >
          <div className="space-y-3">
            <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-[#1F51FF]/15 border border-[#1F51FF]/30 text-[#60A5FA] font-mono text-xs">
              <ShieldCheck className="h-3.5 w-3.5" />
              <span>PCI-DSS 4.0 Level 1 Service Provider</span>
            </div>
            <h1 className="text-3xl sm:text-5xl font-extrabold tracking-tight text-white leading-tight">
              Enterprise Payment Mesh for India&apos;s Largest Platforms
            </h1>
            <p className="text-sm sm:text-base text-slate-300 leading-relaxed font-sans">
              Sign in to manage resilient payment routing across Razorpay, PhonePe, Paytm, and Cashfree. Zero unverified mutations, continuous cryptographic failover, and hardware-backed secret management.
            </p>
          </div>

          {/* Key Enterprise Differentiators */}
          <div className="space-y-3 font-mono text-xs">
            {[
              {
                title: 'Autonomous Sub-Second Failover',
                desc: 'Dynamically shifts traffic within 85ms when upstream bank gateways degrade or fail.',
              },
              {
                title: 'Strict NIST 800-63B Credential Policy',
                desc: 'HSM-enforced 256-bit key storage with rigorous client credential verification.',
              },
              {
                title: 'Multi-Rail Liquidity Optimization',
                desc: 'Unified reconciliation, Webhook fanout, and verified settlement audit trails.',
              },
            ].map((feature, idx) => (
              <div
                key={idx}
                className="p-3.5 rounded-2xl bg-[#0E1424]/80 border border-white/[0.08] flex items-start gap-3"
              >
                <CheckCircle2 className="h-4 w-4 text-emerald-400 shrink-0 mt-0.5" />
                <div>
                  <h4 className="font-bold text-white text-xs">{feature.title}</h4>
                  <p className="text-[11px] text-slate-400 mt-0.5 leading-normal">{feature.desc}</p>
                </div>
              </div>
            ))}
          </div>

          {/* Infrastructure Guarantee Badges */}
          <div className="pt-2 flex flex-wrap items-center gap-4 text-xs font-mono text-slate-400">
            <div className="flex items-center gap-1.5">
              <Lock className="h-3.5 w-3.5 text-blue-400" />
              <span>TLS 1.3 / AES-256-GCM</span>
            </div>
            <div className="flex items-center gap-1.5">
              <Server className="h-3.5 w-3.5 text-emerald-400" />
              <span>99.999% SLA Uptime</span>
            </div>
            <div className="flex items-center gap-1.5">
              <Activity className="h-3.5 w-3.5 text-purple-400" />
              <span>RBI Directive Compliant</span>
            </div>
          </div>
        </motion.div>

        {/* Right Column: Premium Enterprise Auth Box */}
        <motion.div
          initial={{ opacity: 0, x: 20 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.5, delay: 0.1 }}
          className="w-full lg:w-1/2 flex items-center justify-center"
        >
          <EnterpriseAuthDialog
            isOpen={true}
            onClose={() => {}}
            initialMode={isSignup ? 'signup' : 'signin'}
            isStandalonePage={true}
          />
        </motion.div>
      </main>

      {/* Footer */}
      <footer className="relative z-10 border-t border-white/[0.06] py-5 px-6 sm:px-12 text-center text-xs font-mono text-slate-500">
        &copy; {new Date().getFullYear()} ESAPay Inc. Enterprise Payment Infrastructure. All rights reserved.
      </footer>
    </div>
  );
}
