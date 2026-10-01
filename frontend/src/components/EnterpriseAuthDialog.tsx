import React, { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import {
  ShieldCheck,
  Eye,
  EyeOff,
  Check,
  X,
  RefreshCw,
  Zap,
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
          `Enterprise Security Policy: ${strength.errors.join(' • ')}. All 5 parameters required.`
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
        setSuccessMsg('Account created successfully! Redirecting to command center...');
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
    <div
      className={cn(
        'w-full text-white flex flex-col lg:flex-row overflow-hidden font-sans relative selection:bg-[#FAEE1C] selection:text-black',
        isStandalonePage
          ? 'min-h-screen bg-[#070A12]'
          : 'max-w-5xl h-full md:h-[680px] bg-[#070A12] border border-white/[0.12] rounded-none md:rounded-3xl shadow-[0_32px_120px_rgba(0,0,0,0.9)]'
      )}
    >
      {/* ==================================================================== */}
      {/* LEFT COLUMN: ENTERPRISE FINANCIAL MESH VISUAL ARTWORK                 */}
      {/* ==================================================================== */}
      <div className="hidden lg:flex lg:w-1/2 relative overflow-hidden flex-col justify-between p-10 sm:p-12 bg-black select-none">
        {/* Background Visual Art */}
        <img
          src="/auth-mesh-visual.jpg"
          alt="ESAPay Autonomous Routing Infrastructure"
          className="absolute inset-0 w-full h-full object-cover object-center filter brightness-95 contrast-105"
        />

        {/* Ambient Dark Overlays for Deep Contrast */}
        <div className="absolute inset-0 bg-gradient-to-t from-[#070A12] via-[#070A12]/30 to-black/50" />
        <div className="absolute inset-0 bg-gradient-to-r from-black/20 via-transparent to-[#070A12]/80" />

        {/* Top Branding on Artwork */}
        <div className="relative z-10 flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <div className="h-9 w-9 rounded-xl bg-gradient-to-br from-[#1F51FF] to-[#0A2680] flex items-center justify-center text-white font-black text-sm lowercase shadow-lg shadow-[#1F51FF]/40 border border-white/20">
              esa
            </div>
            <span className="font-extrabold text-xl tracking-tight text-white font-sans lowercase">
              esapay
            </span>
            <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-white/10 text-slate-300 border border-white/10 ml-1">
              ENTERPRISE
            </span>
          </div>
        </div>

        {/* Bottom Infrastructure Caption */}
        <div className="relative z-10 space-y-4 max-w-md">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-[#1F51FF]/25 border border-[#1F51FF]/40 text-[#60A5FA] text-xs font-mono backdrop-blur-md">
            <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
            <span>Autonomous Failover Mesh • Live</span>
          </div>

          <h3 className="text-2xl sm:text-3xl font-bold tracking-tight text-white leading-tight font-sans">
            Zero-downtime payment routing across India&apos;s primary banking rails.
          </h3>

          <p className="text-xs text-slate-300/90 leading-relaxed font-sans">
            Continuous sub-85ms cryptographic failover between Razorpay, PhonePe, Paytm, and Cashfree with hardware-enforced secret isolation.
          </p>

          <div className="flex items-center gap-3 text-[11px] font-mono text-slate-400 pt-2 border-t border-white/10">
            <span>85ms Failover</span>
            <span className="text-white/20">•</span>
            <span>PCI-DSS 4.0</span>
            <span className="text-white/20">•</span>
            <span>99.999% SLA Uptime</span>
          </div>
        </div>
      </div>

      {/* ==================================================================== */}
      {/* RIGHT COLUMN: CLEAN ENTERPRISE AUTHENTICATION FORM                   */}
      {/* ==================================================================== */}
      <div className="w-full lg:w-1/2 flex flex-col justify-between p-6 sm:p-10 lg:p-14 overflow-y-auto relative bg-[#070A12]">
        {/* Top Bar / Close Button */}
        <div className="flex items-center justify-between mb-6">
          {/* Mobile wordmark (visible only when left panel is hidden) */}
          <div className="flex lg:hidden items-center gap-2">
            <div className="h-8 w-8 rounded-lg bg-gradient-to-br from-[#1F51FF] to-[#0A2680] flex items-center justify-center text-white font-black text-xs lowercase shadow-md border border-white/20">
              esa
            </div>
            <span className="font-bold text-lg tracking-tight text-white font-sans lowercase">
              esapay
            </span>
          </div>

          {/* Desktop/Modal Close Button */}
          {!isStandalonePage && (
            <button
              onClick={onClose}
              className="ml-auto p-2 rounded-full bg-white/5 hover:bg-white/15 text-slate-400 hover:text-white transition-all cursor-pointer border border-white/5"
              title="Close"
            >
              <X className="h-4 w-4" />
            </button>
          )}
        </div>

        {/* Central Form Container */}
        <div className="my-auto w-full max-w-sm mx-auto space-y-6">
          {/* Heading */}
          <div className="space-y-1.5 text-center lg:text-left">
            <h2 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white font-sans">
              {mode === 'signin' ? 'Access to ESAPay' : 'Create an account'}
            </h2>
            <p className="text-xs sm:text-sm text-slate-400 font-sans">
              {mode === 'signin'
                ? 'Sign in to access your enterprise merchant console'
                : 'Start processing payments with autonomous failover'}
            </p>
          </div>

          {/* Notification Feedback */}
          {errorMsg && (
            <motion.div
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              className="p-3.5 rounded-xl bg-rose-500/10 border border-rose-500/30 text-rose-300 text-xs flex items-start gap-2.5 font-sans"
            >
              <ShieldAlert className="h-4 w-4 shrink-0 text-rose-400 mt-0.5" />
              <div className="text-xs leading-relaxed">{errorMsg}</div>
            </motion.div>
          )}

          {successMsg && (
            <motion.div
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              className="p-3.5 rounded-xl bg-emerald-500/10 border border-emerald-500/30 text-emerald-300 text-xs flex items-center gap-2.5 font-sans"
            >
              <Check className="h-4 w-4 shrink-0 text-emerald-400" />
              <span className="text-xs font-medium">{successMsg}</span>
            </motion.div>
          )}

          {/* Form */}
          <form onSubmit={handleSubmit} className="space-y-4">
            {/* Email Field */}
            <div className="space-y-1.5 text-left">
              <label className="text-xs font-medium text-slate-300 font-sans">Email</label>
              <input
                type="email"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="Insert email"
                className="w-full px-4 py-3 rounded-xl border border-white/10 bg-[#0E1320] text-white placeholder:text-slate-500 text-sm focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner font-sans"
              />
            </div>

            {/* Legal Entity / Organization (Signup Only) */}
            {mode === 'signup' && (
              <div className="space-y-1.5 text-left">
                <label className="text-xs font-medium text-slate-300 font-sans">
                  Organization / Entity
                </label>
                <input
                  type="text"
                  required
                  value={organization}
                  onChange={(e) => setOrganization(e.target.value)}
                  placeholder="e.g. Acme FinTech Private Limited"
                  className="w-full px-4 py-3 rounded-xl border border-white/10 bg-[#0E1320] text-white placeholder:text-slate-500 text-sm focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner font-sans"
                />
              </div>
            )}

            {/* Password Field */}
            <div className="space-y-1.5 text-left">
              <div className="flex items-center justify-between">
                <label className="text-xs font-medium text-slate-300 font-sans">Password</label>
                {mode === 'signin' && (
                  <button
                    type="button"
                    onClick={() => {
                      setErrorMsg(null);
                      setSuccessMsg(
                        'To reset your credentials, reach out to your administrator or support@esapay.internal.'
                      );
                    }}
                    className="text-xs text-slate-400 hover:text-white transition-colors underline underline-offset-2 cursor-pointer font-sans"
                  >
                    Forgot password?
                  </button>
                )}
              </div>
              <div className="relative">
                <input
                  type={showPassword ? 'text' : 'password'}
                  required
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder="Your password"
                  className="w-full pl-4 pr-11 py-3 rounded-xl border border-white/10 bg-[#0E1320] text-white placeholder:text-slate-500 text-sm focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner font-sans"
                />
                <button
                  type="button"
                  onClick={() => setShowPassword(!showPassword)}
                  className="absolute right-3.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-white transition-colors cursor-pointer"
                  title={showPassword ? 'Hide password' : 'Show password'}
                >
                  {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>

              {/* Password Quality Policy Indicators (Signup Only) */}
              {mode === 'signup' && password.length > 0 && (
                <div className="pt-2 space-y-1.5">
                  <div className="flex items-center justify-between text-[11px] font-sans">
                    <span className="text-slate-400">Password Compliance:</span>
                    <span className="font-semibold" style={{ color: strength.color }}>
                      {strength.label} ({strength.percentage}%)
                    </span>
                  </div>
                  <div className="grid grid-cols-2 gap-1 text-[10px] font-mono">
                    {[
                      { label: '8+ chars', met: strength.criteria.minLength },
                      { label: 'Upper (A-Z)', met: strength.criteria.hasUppercase },
                      { label: 'Lower (a-z)', met: strength.criteria.hasLowercase },
                      { label: 'Number (0-9)', met: strength.criteria.hasNumber },
                      { label: 'Symbol (!@#)', met: strength.criteria.hasSpecial },
                    ].map((item, idx) => (
                      <div
                        key={idx}
                        className={cn(
                          'flex items-center gap-1 px-2 py-0.5 rounded text-[10px]',
                          item.met
                            ? 'text-emerald-400 bg-emerald-500/10'
                            : 'text-slate-500 bg-slate-900/60'
                        )}
                      >
                        {item.met ? <Check className="h-3 w-3" /> : <span>•</span>}
                        <span>{item.label}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>

            {/* Confirm Password Field (Signup Only) */}
            {mode === 'signup' && (
              <div className="space-y-1.5 text-left">
                <div className="flex items-center justify-between">
                  <label className="text-xs font-medium text-slate-300 font-sans">
                    Confirm Password
                  </label>
                  {confirmPassword && (
                    <span
                      className={cn(
                        'text-[10px] font-bold font-mono',
                        passwordsMatch ? 'text-emerald-400' : 'text-rose-400'
                      )}
                    >
                      {passwordsMatch ? '✓ Verified' : '✕ Mismatch'}
                    </span>
                  )}
                </div>
                <div className="relative">
                  <input
                    type={showConfirmPassword ? 'text' : 'password'}
                    required
                    value={confirmPassword}
                    onChange={(e) => setConfirmPassword(e.target.value)}
                    placeholder="Re-enter your password"
                    className="w-full pl-4 pr-11 py-3 rounded-xl border border-white/10 bg-[#0E1320] text-white placeholder:text-slate-500 text-sm focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner font-sans"
                  />
                  <button
                    type="button"
                    onClick={() => setShowConfirmPassword(!showConfirmPassword)}
                    className="absolute right-3.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-white transition-colors cursor-pointer"
                  >
                    {showConfirmPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
              </div>
            )}

            {/* Primary Action Pill Button */}
            <div className="pt-2">
              <button
                type="submit"
                disabled={isLoading || (mode === 'signup' && (!strength.isValid || !passwordsMatch))}
                className={cn(
                  'w-full py-3.5 px-8 rounded-full font-bold text-sm tracking-wide transition-all shadow-xl flex items-center justify-center gap-2 cursor-pointer font-sans',
                  mode === 'signup' && (!strength.isValid || !passwordsMatch)
                    ? 'bg-slate-800 text-slate-500 cursor-not-allowed border border-white/5'
                    : 'bg-[#FAEE1C] hover:bg-[#F3E708] text-black shadow-amber-400/20 hover:scale-[1.01] active:scale-[0.99]'
                )}
              >
                {isLoading ? (
                  <>
                    <RefreshCw className="h-4 w-4 animate-spin text-black" />
                    <span>Verifying session...</span>
                  </>
                ) : (
                  <span>{mode === 'signin' ? 'Login' : 'Create Account'}</span>
                )}
              </button>
            </div>
          </form>

          {/* Hairline Divider */}
          <div className="border-t border-white/[0.08] my-6" />

          {/* Account Mode Switcher */}
          <div className="text-center text-xs sm:text-sm text-slate-400 font-sans">
            {mode === 'signin' ? (
              <span>
                New to ESAPay?{' '}
                <button
                  type="button"
                  onClick={() => {
                    setMode('signup');
                    setErrorMsg(null);
                    setSuccessMsg(null);
                  }}
                  className="text-white hover:text-[#FAEE1C] font-semibold underline underline-offset-4 cursor-pointer transition-colors"
                >
                  Create an account
                </button>
              </span>
            ) : (
              <span>
                Already have an account?{' '}
                <button
                  type="button"
                  onClick={() => {
                    setMode('signin');
                    setErrorMsg(null);
                    setSuccessMsg(null);
                  }}
                  className="text-white hover:text-[#FAEE1C] font-semibold underline underline-offset-4 cursor-pointer transition-colors"
                >
                  Sign in
                </button>
              </span>
            )}
          </div>

          {/* Secondary Actions: Enterprise SSO & Sandbox Link */}
          <div className="pt-4 space-y-2.5 text-center">
            <div>
              <button
                type="button"
                onClick={handleSsoClick}
                className="inline-flex items-center gap-1.5 text-xs text-slate-500 hover:text-slate-300 transition-colors cursor-pointer group"
              >
                <ShieldCheck className="h-3.5 w-3.5 text-[#60A5FA]" />
                <span>Continue with Enterprise SSO (SAML 2.0 / Okta)</span>
              </button>
            </div>

            {ssoInfoOpen && (
              <motion.div
                initial={{ opacity: 0, y: -4 }}
                animate={{ opacity: 1, y: 0 }}
                className="p-3.5 rounded-xl bg-[#121826] border border-[#1F51FF]/30 text-xs flex items-start gap-2.5 text-left font-sans shadow-lg"
              >
                <ShieldCheck className="h-4 w-4 shrink-0 text-[#60A5FA] mt-0.5" />
                <div className="space-y-1 flex-1">
                  <div className="flex items-center justify-between">
                    <strong className="text-white text-[11px]">Corporate SAML / Okta SSO</strong>
                    <button
                      type="button"
                      onClick={() => setSsoInfoOpen(false)}
                      className="text-slate-400 hover:text-white text-[10px] cursor-pointer"
                    >
                      ✕ Close
                    </button>
                  </div>
                  <p className="text-[11px] leading-relaxed text-slate-300">
                    Corporate SSO requires domain DNS verification. For direct evaluation, please sign in or create an account with your <strong>Work Email & Password</strong> above.
                  </p>
                </div>
              </motion.div>
            )}

            <div>
              <a
                href="/dashboard?mode=sandbox"
                className="inline-flex items-center gap-1 text-xs text-amber-400/90 hover:text-amber-300 font-medium transition-colors"
              >
                <Zap className="h-3 w-3 text-amber-400" />
                <span>Evaluating? Open Instant Sandbox (No Login) &rarr;</span>
              </a>
            </div>
          </div>
        </div>

        {/* Bottom Micro Footer */}
        <div className="pt-6 text-center text-[10px] font-mono text-slate-600">
          TLS 1.3 • AES-256-GCM • Backed by Autonomous Failover Engine
        </div>
      </div>
    </div>
  );

  if (isStandalonePage) {
    return content;
  }

  return (
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-0 md:p-6 lg:p-10 bg-black/85 backdrop-blur-2xl overflow-y-auto">
          <motion.div
            initial={{ opacity: 0, scale: 0.96, y: 15 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.96, y: 15 }}
            transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
            className="w-full max-w-5xl my-auto"
          >
            {content}
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}
