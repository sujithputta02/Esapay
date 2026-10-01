import React, { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import {
  ShieldCheck,
  Lock,
  Building2,
  Mail,
  ArrowRight,
  Eye,
  EyeOff,
  Check,
  X,
  RefreshCw,
  Zap,
  Server,
  KeyRound,
  ShieldAlert,
} from 'lucide-react';
import { supabaseAuth, evaluatePasswordStrength, UserSession } from '@/lib/supabase';
import { cn } from '@/lib/utils';

interface EnterpriseAuthDialogProps {
  isOpen: boolean;
  onClose: () => void;
  initialMode?: 'signin' | 'signup';
  onSuccess?: (session: UserSession) => void;
  isStandalonePage?: boolean;
}

export function EnterpriseAuthDialog({
  isOpen,
  onClose,
  initialMode = 'signin',
  onSuccess,
  isStandalonePage = false,
}: EnterpriseAuthDialogProps) {
  const [mode, setMode] = useState<'signin' | 'signup'>(initialMode);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [organization, setOrganization] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirmPassword, setShowConfirmPassword] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);
  const [ssoInfoOpen, setSsoInfoOpen] = useState(false);

  useEffect(() => {
    setMode(initialMode);
    setErrorMsg(null);
    setSuccessMsg(null);
    setSsoInfoOpen(false);
  }, [initialMode, isOpen]);

  const strength = evaluatePasswordStrength(password);
  const passwordsMatch = password.length > 0 && password === confirmPassword;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setErrorMsg(null);
    setSuccessMsg(null);

    const trimmedEmail = email.trim();
    if (!trimmedEmail) {
      setErrorMsg('Please specify a valid corporate work email address.');
      return;
    }
    if (!password) {
      setErrorMsg('Please enter your root enterprise credentials.');
      return;
    }

    if (mode === 'signup') {
      if (!strength.isValid) {
        setErrorMsg(
          `Enterprise Security Policy Unmet: ${strength.errors.join(' • ')}. All 5 security parameters must be satisfied.`
        );
        return;
      }
      if (password !== confirmPassword) {
        setErrorMsg('Password confirmation does not match the entered password.');
        return;
      }
      if (!organization.trim()) {
        setErrorMsg('Please enter your legal merchant entity or organization name.');
        return;
      }
    } else {
      if (password.length < 6) {
        setErrorMsg('Password must be at least 6 characters.');
        return;
      }
    }

    setIsLoading(true);
    try {
      let session: UserSession;
      if (mode === 'signup') {
        session = await supabaseAuth.signUp(
          trimmedEmail,
          password,
          organization.trim() || 'Enterprise Workspace'
        );
        setSuccessMsg('Entity provisioned successfully! Initializing command center...');
      } else {
        session = await supabaseAuth.signIn(trimmedEmail, password);
        setSuccessMsg('Authentication verified. Loading merchant cluster...');
      }

      if (onSuccess) {
        onSuccess(session);
      }

      setTimeout(() => {
        if (!isStandalonePage) {
          onClose();
        }
        if (mode === 'signup') {
          window.location.href = '/keys?welcome=1';
        } else {
          window.location.href = '/dashboard?mode=live';
        }
      }, 700);
    } catch (err: any) {
      setErrorMsg(err.message || 'Authentication rejected by security policy.');
    } finally {
      setIsLoading(false);
    }
  };

  const handleSsoClick = () => {
    setSsoInfoOpen((prev) => !prev);
    setErrorMsg(null);
  };

  const content = (
    <div className="w-full max-w-xl mx-auto bg-[#0B0F19]/95 backdrop-blur-2xl border border-white/[0.12] rounded-3xl shadow-[0_32px_120px_rgba(0,0,0,0.85)] text-white relative max-h-[92vh] flex flex-col overflow-hidden">
      {/* Top Subtle Ambient Brand Glow */}
      <div className="pointer-events-none absolute -top-24 left-1/2 -translate-x-1/2 w-96 h-48 bg-[#1F51FF]/25 blur-[90px] rounded-full" />
      <div className="pointer-events-none absolute -bottom-24 right-0 w-64 h-48 bg-emerald-500/10 blur-[80px] rounded-full" />

      {/* Header Bar */}
      <div className="px-6 sm:px-8 pt-7 pb-5 border-b border-white/[0.08] relative z-10 flex items-start justify-between">
        <div className="space-y-1.5">
          <div className="flex items-center gap-2.5">
            <div className="h-9 w-9 rounded-xl bg-gradient-to-br from-[#1F51FF] to-[#1236B5] flex items-center justify-center text-white font-extrabold text-sm shadow-lg shadow-[#1F51FF]/30 lowercase border border-white/20">
              esa
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h3 className="font-extrabold text-lg sm:text-xl tracking-tight text-white">
                  {mode === 'signup' ? 'Provision Merchant Entity' : 'Enterprise Merchant Sign In'}
                </h3>
                <span className="hidden sm:inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-mono font-bold bg-[#1F51FF]/20 text-[#60A5FA] border border-[#1F51FF]/40">
                  <ShieldCheck className="h-3 w-3" />
                  PCI-DSS 4.0
                </span>
              </div>
              <p className="text-xs text-slate-400 font-mono">
                {mode === 'signup'
                  ? 'Autonomous routing mesh & cryptographically signed keys'
                  : 'Access authenticated Live Command Center & Gateway Mesh'}
              </p>
            </div>
          </div>
        </div>

        {!isStandalonePage && (
          <button
            onClick={onClose}
            className="h-8 w-8 rounded-full bg-white/5 hover:bg-white/10 text-slate-400 hover:text-white flex items-center justify-center transition-colors cursor-pointer border border-white/5"
            title="Close"
          >
            <X className="h-4 w-4" />
          </button>
        )}
      </div>

      {/* Security Compliance Micro-Bar */}
      <div className="bg-[#070A12] px-6 sm:px-8 py-2 border-b border-white/[0.06] flex items-center justify-between text-[11px] font-mono text-slate-400 overflow-x-auto no-scrollbar">
        <div className="flex items-center gap-3">
          <span className="flex items-center gap-1.5 text-emerald-400 font-semibold">
            <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse" />
            HSM-256 GCM
          </span>
          <span className="text-white/20">|</span>
          <span>SOC2 Type II</span>
          <span className="text-white/20">|</span>
          <span>RBI Compliant</span>
        </div>
        <div className="flex items-center gap-1.5 text-slate-400">
          <Server className="h-3 w-3 text-blue-400" />
          <span>ap-south-1 (Mumbai)</span>
        </div>
      </div>

      <div className="p-5 sm:p-8 space-y-5 sm:space-y-6 relative z-10 overflow-y-auto">
        {/* Mode Switcher Tabs */}
        <div className="p-1 rounded-2xl bg-[#121826] border border-white/[0.08] flex items-center gap-1 font-mono text-xs">
          <button
            type="button"
            onClick={() => {
              setMode('signin');
              setErrorMsg(null);
            }}
            className={cn(
              'flex-1 py-2.5 rounded-xl font-bold transition-all flex items-center justify-center gap-2 cursor-pointer',
              mode === 'signin'
                ? 'bg-gradient-to-r from-[#1F51FF] to-[#2563EB] text-white shadow-lg shadow-[#1F51FF]/30 border border-white/20'
                : 'text-slate-400 hover:text-white hover:bg-white/[0.04]'
            )}
          >
            <KeyRound className="h-3.5 w-3.5" />
            <span>Sign In</span>
          </button>
          <button
            type="button"
            onClick={() => {
              setMode('signup');
              setErrorMsg(null);
            }}
            className={cn(
              'flex-1 py-2.5 rounded-xl font-bold transition-all flex items-center justify-center gap-2 cursor-pointer',
              mode === 'signup'
                ? 'bg-gradient-to-r from-[#1F51FF] to-[#2563EB] text-white shadow-lg shadow-[#1F51FF]/30 border border-white/20'
                : 'text-slate-400 hover:text-white hover:bg-white/[0.04]'
            )}
          >
            <Building2 className="h-3.5 w-3.5" />
            <span>Provision Entity</span>
          </button>
        </div>

        {/* Feedback Notifications */}
        {errorMsg && (
          <motion.div
            initial={{ opacity: 0, y: -5 }}
            animate={{ opacity: 1, y: 0 }}
            className="p-3.5 rounded-2xl bg-rose-500/10 border border-rose-500/30 text-rose-300 text-xs flex items-start gap-2.5 font-mono"
          >
            <ShieldAlert className="h-4 w-4 shrink-0 text-rose-400 mt-0.5" />
            <div className="space-y-0.5">
              <strong className="block text-rose-200">Security Requirement Alert:</strong>
              <p className="text-[11px] leading-relaxed text-rose-300/90">{errorMsg}</p>
            </div>
          </motion.div>
        )}

        {successMsg && (
          <motion.div
            initial={{ opacity: 0, y: -5 }}
            animate={{ opacity: 1, y: 0 }}
            className="p-3.5 rounded-2xl bg-emerald-500/10 border border-emerald-500/30 text-emerald-300 text-xs flex items-center gap-2.5 font-mono"
          >
            <Check className="h-4 w-4 shrink-0 text-emerald-400" />
            <span>{successMsg}</span>
          </motion.div>
        )}

        {/* Form */}
        <form onSubmit={handleSubmit} className="space-y-4 font-mono">
          {/* Work Email */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-[11px] font-bold uppercase tracking-wider text-slate-300 flex items-center gap-1.5">
                <Mail className="h-3 w-3 text-blue-400" />
                <span>Corporate Work Email</span>
              </label>
              <span className="text-[10px] text-slate-500">Domain-verified</span>
            </div>
            <div className="relative">
              <input
                type="email"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="name@company.com"
                className="w-full px-4 py-2.5 rounded-xl border border-white/[0.12] bg-[#121826]/90 text-white placeholder:text-slate-500 text-xs focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner"
              />
            </div>
          </div>

          {/* Legal Entity / Organization (Signup only) */}
          {mode === 'signup' && (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <label className="text-[11px] font-bold uppercase tracking-wider text-slate-300 flex items-center gap-1.5">
                  <Building2 className="h-3 w-3 text-emerald-400" />
                  <span>Legal Merchant Entity</span>
                </label>
                <span className="text-[10px] text-slate-500">KYC & Invoice</span>
              </div>
              <input
                type="text"
                required
                value={organization}
                onChange={(e) => setOrganization(e.target.value)}
                placeholder="e.g. Acme FinTech Private Limited"
                className="w-full px-4 py-2.5 rounded-xl border border-white/[0.12] bg-[#121826]/90 text-white placeholder:text-slate-500 text-xs focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner"
              />
            </div>
          )}

          {/* Password Input */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-[11px] font-bold uppercase tracking-wider text-slate-300 flex items-center gap-1.5">
                <Lock className="h-3 w-3 text-amber-400" />
                <span>{mode === 'signup' ? 'Root Security Credential' : 'Password'}</span>
              </label>
              {mode === 'signup' && (
                <span className="text-[10px] text-amber-400 font-semibold">Strict NIST Policy</span>
              )}
            </div>
            <div className="relative">
              <input
                type={showPassword ? 'text' : 'password'}
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={
                  mode === 'signup'
                    ? '8+ chars, upper, lower, number, symbol'
                    : 'Enter your account password'
                }
                className="w-full pl-4 pr-10 py-2.5 rounded-xl border border-white/[0.12] bg-[#121826]/90 text-white placeholder:text-slate-500 text-xs focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner"
              />
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-slate-400 hover:text-white transition-colors cursor-pointer"
                title={showPassword ? 'Hide password' : 'Show password'}
              >
                {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            </div>

            {/* REAL-TIME ENTERPRISE PASSWORD STRENGTH EVALUATOR */}
            {mode === 'signup' && password.length > 0 && (
              <div className="mt-2.5 p-3 rounded-2xl bg-[#070A12] border border-white/[0.08] space-y-2.5">
                <div className="flex items-center justify-between text-[11px]">
                  <span className="text-slate-400">Password Policy Score:</span>
                  <span className="font-bold flex items-center gap-1.5" style={{ color: strength.color }}>
                    <span className="h-2 w-2 rounded-full" style={{ backgroundColor: strength.color }} />
                    {strength.label} ({strength.percentage}% Compliance)
                  </span>
                </div>

                {/* Segmented meter bar */}
                <div className="grid grid-cols-5 gap-1.5">
                  {[1, 2, 3, 4, 5].map((seg) => (
                    <div
                      key={seg}
                      className={cn(
                        'h-1.5 rounded-full transition-all duration-300',
                        strength.score >= seg ? 'opacity-100' : 'bg-slate-800 opacity-40'
                      )}
                      style={{
                        backgroundColor: strength.score >= seg ? strength.color : undefined,
                      }}
                    />
                  ))}
                </div>

                {/* 5-parameter criteria matrix */}
                <div className="grid grid-cols-2 sm:grid-cols-3 gap-1.5 pt-1 text-[10px]">
                  {[
                    { label: '8+ Characters', met: strength.criteria.minLength },
                    { label: 'Uppercase (A-Z)', met: strength.criteria.hasUppercase },
                    { label: 'Lowercase (a-z)', met: strength.criteria.hasLowercase },
                    { label: 'Numeric (0-9)', met: strength.criteria.hasNumber },
                    { label: 'Special Symbol (!@#$)', met: strength.criteria.hasSpecial },
                  ].map((item, idx) => (
                    <div
                      key={idx}
                      className={cn(
                        'flex items-center gap-1 px-2 py-1 rounded-md border text-[10px] transition-colors',
                        item.met
                          ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-300 font-semibold'
                          : 'bg-slate-900 border-slate-800 text-slate-500'
                      )}
                    >
                      {item.met ? (
                        <Check className="h-3 w-3 text-emerald-400 shrink-0" />
                      ) : (
                        <span className="h-1.5 w-1.5 rounded-full bg-slate-600 shrink-0" />
                      )}
                      <span className="truncate">{item.label}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>

          {/* Confirm Password (Signup only) */}
          {mode === 'signup' && (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <label className="text-[11px] font-bold uppercase tracking-wider text-slate-300 flex items-center gap-1.5">
                  <Lock className="h-3 w-3 text-amber-400" />
                  <span>Confirm Security Credential</span>
                </label>
                {confirmPassword && (
                  <span
                    className={cn(
                      'text-[10px] font-bold flex items-center gap-1',
                      passwordsMatch ? 'text-emerald-400' : 'text-rose-400'
                    )}
                  >
                    {passwordsMatch ? '✓ Verified match' : '✕ Mismatch'}
                  </span>
                )}
              </div>
              <div className="relative">
                <input
                  type={showConfirmPassword ? 'text' : 'password'}
                  required
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  placeholder="Re-enter root credential"
                  className="w-full pl-4 pr-10 py-2.5 rounded-xl border border-white/[0.12] bg-[#121826]/90 text-white placeholder:text-slate-500 text-xs focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner"
                />
                <button
                  type="button"
                  onClick={() => setShowConfirmPassword(!showConfirmPassword)}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-slate-400 hover:text-white transition-colors cursor-pointer"
                  title={showConfirmPassword ? 'Hide password' : 'Show password'}
                >
                  {showConfirmPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
            </div>
          )}

          {/* Submit Action Button */}
          <div className="pt-2">
            <button
              type="submit"
              disabled={isLoading || (mode === 'signup' && (!strength.isValid || !passwordsMatch))}
              className={cn(
                'w-full py-3.5 px-6 rounded-2xl font-bold text-xs flex items-center justify-center gap-2 shadow-xl transition-all cursor-pointer font-mono tracking-wide',
                mode === 'signup' && (!strength.isValid || !passwordsMatch)
                  ? 'bg-slate-800 text-slate-500 cursor-not-allowed border border-white/5'
                  : 'bg-gradient-to-r from-[#1F51FF] via-[#2A5CFF] to-[#1644DF] hover:brightness-110 text-white shadow-[#1F51FF]/30 hover:scale-[1.01] active:scale-[0.99] border border-white/20'
              )}
            >
              {isLoading ? (
                <>
                  <RefreshCw className="h-4 w-4 animate-spin" />
                  <span>Verifying Cryptographic Tokens...</span>
                </>
              ) : mode === 'signup' ? (
                <>
                  <ShieldCheck className="h-4 w-4" />
                  <span>Provision Merchant Entity & Access Console</span>
                  <ArrowRight className="h-4 w-4" />
                </>
              ) : (
                <>
                  <KeyRound className="h-4 w-4" />
                  <span>Authenticate & Open Live Command Center</span>
                  <ArrowRight className="h-4 w-4" />
                </>
              )}
            </button>
          </div>
        </form>

        {/* Enterprise SAML / Okta SSO Secondary Option */}
        <div className="pt-1 text-center">
          <button
            type="button"
            onClick={handleSsoClick}
            className="inline-flex items-center gap-2 text-[11px] font-mono text-slate-400 hover:text-white transition-colors cursor-pointer group py-1 px-2.5 rounded-lg hover:bg-white/[0.04]"
          >
            <ShieldCheck className="h-3.5 w-3.5 text-[#60A5FA] group-hover:scale-110 transition-transform" />
            <span>Enterprise SAML 2.0 / Okta SSO</span>
            <span className="px-1.5 py-0.5 rounded text-[9px] font-bold bg-[#1F51FF]/20 text-[#60A5FA] border border-[#1F51FF]/30">
              Enterprise Only
            </span>
          </button>
        </div>

        {ssoInfoOpen && (
          <motion.div
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            className="p-3.5 rounded-2xl bg-[#121826] border border-[#1F51FF]/30 text-xs flex items-start gap-2.5 font-mono shadow-lg"
          >
            <ShieldCheck className="h-4 w-4 shrink-0 text-[#60A5FA] mt-0.5" />
            <div className="space-y-1 text-left flex-1">
              <div className="flex items-center justify-between">
                <strong className="text-white text-[11px]">Corporate SAML 2.0 & Okta SSO</strong>
                <button
                  type="button"
                  onClick={() => setSsoInfoOpen(false)}
                  className="text-slate-400 hover:text-white text-[10px] cursor-pointer"
                >
                  ✕ Close
                </button>
              </div>
              <p className="text-[11px] leading-relaxed text-slate-300">
                Corporate SSO requires domain DNS verification and identity provider configuration. For immediate access, please sign in or provision your account using your <strong>Work Email & Password</strong> above.
              </p>
            </div>
          </motion.div>
        )}

        {/* Developer Sandbox Alternative Banner */}
        <div className="pt-3 border-t border-white/[0.08] flex flex-col sm:flex-row items-center justify-between gap-3 text-xs font-mono">
          <div className="flex items-center gap-2 text-slate-400 text-[11px]">
            <Zap className="h-3.5 w-3.5 text-amber-400" />
            <span>Evaluating or running automated tests?</span>
          </div>
          <a
            href="/dashboard?mode=sandbox"
            className="px-3 py-1 rounded-full bg-amber-400/10 hover:bg-amber-400/20 border border-amber-400/30 text-amber-300 hover:text-amber-200 font-bold text-[11px] flex items-center gap-1.5 transition-all shadow-sm"
          >
            <span>Instant Sandbox Console (No Login)</span>
            <ArrowRight className="h-3 w-3" />
          </a>
        </div>
      </div>

      {/* Security Footer Note */}
      <div className="px-6 sm:px-8 py-3 bg-[#070A12] border-t border-white/[0.06] text-center text-[10px] font-mono text-slate-500">
        End-to-End Encrypted via TLS 1.3 / AES-256-GCM. Backed by ESAPay Autonomous Failover Engine.
      </div>
    </div>
  );

  if (isStandalonePage) {
    return content;
  }

  return (
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-6 bg-black/85 backdrop-blur-md overflow-y-auto">
          <motion.div
            initial={{ opacity: 0, scale: 0.94, y: 15 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.94, y: 15 }}
            transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            className="w-full max-w-xl my-auto"
          >
            {content}
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}
