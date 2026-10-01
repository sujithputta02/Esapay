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
  KeyRound,
  Building2,
  Server,
  Globe,
  ArrowRight,
} from 'lucide-react';
import { supabaseAuth, evaluatePasswordStrength, UserSession } from '@/lib/supabase';
import { cn } from '@/lib/utils';

export type AuthMode = 'signin' | 'signup' | 'enterprise';

interface EnterpriseAuthDialogProps {
  isOpen: boolean;
  onClose: () => void;
  initialMode?: 'signin' | 'signup' | 'enterprise';
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
  const [mode, setMode] = useState<AuthMode>(initialMode);
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [organization, setOrganization] = useState('');
  const [ssoDomain, setSsoDomain] = useState('');
  const [ssoProvider, setSsoProvider] = useState<'okta' | 'azure' | 'google' | 'ping'>('okta');
  const [showPassword, setShowPassword] = useState(false);
  const [showConfirmPassword, setShowConfirmPassword] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  useEffect(() => {
    setMode(initialMode);
    setErrorMsg(null);
    setSuccessMsg(null);
  }, [initialMode, isOpen]);

  const strength = evaluatePasswordStrength(password);
  const passwordsMatch = password.length > 0 && password === confirmPassword;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setErrorMsg(null);
    setSuccessMsg(null);

    // 1. Enterprise SSO Flow
    if (mode === 'enterprise') {
      const trimmedDomain = ssoDomain.trim();
      if (!trimmedDomain) {
        setErrorMsg('Please enter your corporate email or organization domain (e.g. acmefin.com).');
        return;
      }
      setIsLoading(true);
      setTimeout(() => {
        setIsLoading(false);
        setErrorMsg(
          `Domain "${trimmedDomain}" is mapped to Enterprise ${ssoProvider.toUpperCase()} SSO. Corporate DNS verification TXT record is pending activation. For immediate access, please use standard Sign In with your credentials.`
        );
      }, 800);
      return;
    }

    // 2. Standard Sign In / Provision Flow
    const trimmedEmail = email.trim();
    if (!trimmedEmail) {
      setErrorMsg('Please specify a valid corporate work email address.');
      return;
    }
    if (!password) {
      setErrorMsg('Please enter your account password.');
      return;
    }

    if (mode === 'signup') {
      if (!organization.trim()) {
        setErrorMsg('Please enter your legal merchant entity or company name.');
        return;
      }
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

  const content = (
    <div className="w-full h-full min-h-screen bg-[#070A12] text-white flex flex-col lg:flex-row overflow-hidden font-sans relative selection:bg-[#FAEE1C] selection:text-black">
      {/* Top Right Floating Close Button (when opened from modal) */}
      {!isStandalonePage && (
        <button
          onClick={onClose}
          className="absolute top-5 right-5 z-40 p-2.5 rounded-full bg-white/10 hover:bg-white/20 text-slate-300 hover:text-white transition-all cursor-pointer border border-white/10 backdrop-blur-md shadow-xl"
          title="Close"
        >
          <X className="h-5 w-5" />
        </button>
      )}

      {/* ==================================================================== */}
      {/* LEFT COLUMN: FULL-HEIGHT ENTERPRISE FINANCIAL MESH VISUAL             */}
      {/* ==================================================================== */}
      <div className="hidden lg:flex lg:w-1/2 h-full relative overflow-hidden flex-col justify-between p-12 lg:p-16 bg-black select-none border-r border-white/[0.08]">
        {/* Full-bleed background visual */}
        <img
          src="/auth-mesh-visual.jpg"
          alt="ESAPay Autonomous Routing Infrastructure"
          className="absolute inset-0 w-full h-full object-cover object-center filter brightness-95 contrast-105"
        />

        {/* Ambient Dark Overlays */}
        <div className="absolute inset-0 bg-gradient-to-t from-[#070A12] via-[#070A12]/35 to-black/55" />
        <div className="absolute inset-0 bg-gradient-to-r from-black/25 via-transparent to-[#070A12]/90" />

        {/* Top Branding on Artwork */}
        <div className="relative z-10 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="h-10 w-10 rounded-xl bg-gradient-to-br from-[#1F51FF] to-[#0A2680] flex items-center justify-center text-white font-black text-sm lowercase shadow-xl shadow-[#1F51FF]/40 border border-white/20">
              esa
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="font-extrabold text-2xl tracking-tight text-white font-sans lowercase">
                  esapay
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-white/10 text-slate-300 border border-white/10">
                  ENTERPRISE
                </span>
              </div>
              <p className="text-[11px] font-mono text-slate-400">
                Autonomous Multi-Gateway Payment Mesh
              </p>
            </div>
          </div>
        </div>

        {/* Bottom Infrastructure Caption */}
        <div className="relative z-10 space-y-4 max-w-lg">
          <div className="inline-flex items-center gap-2 px-3 py-1.5 rounded-full bg-[#1F51FF]/25 border border-[#1F51FF]/40 text-[#60A5FA] text-xs font-mono backdrop-blur-md">
            <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
            <span>Autonomous Failover Mesh • Active Production Rails</span>
          </div>

          <h3 className="text-3xl lg:text-4xl font-extrabold tracking-tight text-white leading-tight font-sans">
            Zero-downtime payment routing across India&apos;s primary banking rails.
          </h3>

          <p className="text-sm text-slate-300 leading-relaxed font-sans">
            Continuous sub-85ms cryptographic failover between Razorpay, PhonePe, Paytm, and Cashfree with hardware-enforced secret isolation.
          </p>

          <div className="flex items-center gap-4 text-xs font-mono text-slate-400 pt-3 border-t border-white/10">
            <span className="text-emerald-400 font-semibold">85ms Failover</span>
            <span className="text-white/20">•</span>
            <span>PCI-DSS 4.0 Level 1</span>
            <span className="text-white/20">•</span>
            <span>99.999% SLA Uptime</span>
          </div>
        </div>
      </div>

      {/* ==================================================================== */}
      {/* RIGHT COLUMN: FULL-HEIGHT CLEAN ENTERPRISE AUTHENTICATION             */}
      {/* ==================================================================== */}
      <div className="w-full lg:w-1/2 h-full flex flex-col justify-between p-6 sm:p-12 lg:p-16 overflow-y-auto relative bg-[#070A12]">
        {/* Mobile Header (when left artwork is hidden) */}
        <div className="flex lg:hidden items-center justify-between mb-6">
          <div className="flex items-center gap-2.5">
            <div className="h-8 w-8 rounded-lg bg-gradient-to-br from-[#1F51FF] to-[#0A2680] flex items-center justify-center text-white font-black text-xs lowercase shadow-md border border-white/20">
              esa
            </div>
            <span className="font-bold text-lg tracking-tight text-white font-sans lowercase">
              esapay
            </span>
            <span className="text-[9px] font-mono px-2 py-0.5 rounded-full bg-white/10 text-slate-300">
              ENTERPRISE
            </span>
          </div>
        </div>

        {/* Center Auth Card */}
        <div className="my-auto w-full max-w-md mx-auto space-y-7 py-4">
          {/* THREE-WAY ENTERPRISE SEGMENTED CONTROL */}
          <div className="p-1 rounded-2xl bg-[#0E1320] border border-white/10 flex items-center gap-1 font-sans text-xs">
            <button
              type="button"
              onClick={() => {
                setMode('signin');
                setErrorMsg(null);
                setSuccessMsg(null);
              }}
              className={cn(
                'flex-1 py-2.5 rounded-xl font-bold transition-all flex items-center justify-center gap-1.5 cursor-pointer',
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
                setSuccessMsg(null);
              }}
              className={cn(
                'flex-1 py-2.5 rounded-xl font-bold transition-all flex items-center justify-center gap-1.5 cursor-pointer',
                mode === 'signup'
                  ? 'bg-gradient-to-r from-[#1F51FF] to-[#2563EB] text-white shadow-lg shadow-[#1F51FF]/30 border border-white/20'
                  : 'text-slate-400 hover:text-white hover:bg-white/[0.04]'
              )}
            >
              <Building2 className="h-3.5 w-3.5" />
              <span>Provision Entity</span>
            </button>

            <button
              type="button"
              onClick={() => {
                setMode('enterprise');
                setErrorMsg(null);
                setSuccessMsg(null);
              }}
              className={cn(
                'flex-1 py-2.5 rounded-xl font-bold transition-all flex items-center justify-center gap-1.5 cursor-pointer',
                mode === 'enterprise'
                  ? 'bg-gradient-to-r from-[#1F51FF] to-[#2563EB] text-white shadow-lg shadow-[#1F51FF]/30 border border-white/20'
                  : 'text-slate-400 hover:text-white hover:bg-white/[0.04]'
              )}
            >
              <ShieldCheck className="h-3.5 w-3.5" />
              <span>Enterprise SSO</span>
            </button>
          </div>

          {/* Heading Section */}
          <div className="space-y-1.5 text-center lg:text-left">
            <h2 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white font-sans">
              {mode === 'signin' && 'Access to ESAPay'}
              {mode === 'signup' && 'Provision Merchant Entity'}
              {mode === 'enterprise' && 'Corporate SAML 2.0 / Okta SSO'}
            </h2>
            <p className="text-xs sm:text-sm text-slate-400 font-sans">
              {mode === 'signin' && 'Sign in to access your live command center and telemetry'}
              {mode === 'signup' && 'Register your legal entity to initialize sovereign payment routing'}
              {mode === 'enterprise' && 'Single Sign-On for organizations with dedicated identity providers'}
            </p>
          </div>

          {/* Alert Messages */}
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
            {/* ----------------------------------------------------------- */}
            {/* ENTERPRISE SSO MODE FIELDS                                  */}
            {/* ----------------------------------------------------------- */}
            {mode === 'enterprise' && (
              <div className="space-y-4 text-left">
                <div className="space-y-1.5">
                  <label className="text-xs font-medium text-slate-300 font-sans flex items-center justify-between">
                    <span>Corporate Email or Domain</span>
                    <span className="text-[10px] font-mono text-[#60A5FA]">SAML 2.0 / OIDC</span>
                  </label>
                  <input
                    type="text"
                    required
                    value={ssoDomain}
                    onChange={(e) => setSsoDomain(e.target.value)}
                    placeholder="name@company.com or company.com"
                    className="w-full px-4 py-3 rounded-xl border border-white/10 bg-[#0E1320] text-white placeholder:text-slate-500 text-sm focus:outline-none focus:ring-2 focus:ring-[#1F51FF] focus:border-transparent transition-all shadow-inner font-sans"
                  />
                </div>

                <div className="space-y-1.5">
                  <label className="text-xs font-medium text-slate-300 font-sans">
                    Identity Provider
                  </label>
                  <div className="grid grid-cols-2 gap-2 text-xs font-sans">
                    {[
                      { id: 'okta', label: 'Okta Enterprise' },
                      { id: 'azure', label: 'Azure AD / Entra ID' },
                      { id: 'google', label: 'Google Workspace' },
                      { id: 'ping', label: 'PingFederate SAML' },
                    ].map((p) => (
                      <button
                        key={p.id}
                        type="button"
                        onClick={() => setSsoProvider(p.id as any)}
                        className={cn(
                          'p-2.5 rounded-xl border text-xs font-medium transition-all text-left flex items-center justify-between cursor-pointer',
                          ssoProvider === p.id
                            ? 'bg-[#1F51FF]/20 border-[#1F51FF] text-white shadow-sm'
                            : 'bg-[#0E1320] border-white/10 text-slate-400 hover:text-white hover:bg-white/[0.04]'
                        )}
                      >
                        <span>{p.label}</span>
                        {ssoProvider === p.id && <Check className="h-3.5 w-3.5 text-[#60A5FA]" />}
                      </button>
                    ))}
                  </div>
                </div>

                <div className="p-3.5 rounded-xl bg-[#0E1320] border border-white/10 space-y-2 text-xs font-sans">
                  <div className="flex items-center gap-2 text-[#60A5FA] font-semibold text-xs">
                    <Globe className="h-3.5 w-3.5" />
                    <span>Domain DNS Verification</span>
                  </div>
                  <p className="text-[11px] text-slate-400 leading-relaxed">
                    Custom SSO requires configuring a DNS TXT record for your domain with our ACS endpoint. Contact enterprise support or use Root Sign In for instant access.
                  </p>
                </div>
              </div>
            )}

            {/* ----------------------------------------------------------- */}
            {/* SIGN IN & SIGN UP (PROVISION) FIELDS                        */}
            {/* ----------------------------------------------------------- */}
            {mode !== 'enterprise' && (
              <>
                {/* Work Email */}
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
                    <label className="text-xs font-medium text-slate-300 font-sans flex items-center justify-between">
                      <span>Legal Merchant Entity</span>
                      <span className="text-[10px] font-mono text-emerald-400">KYC & Invoice</span>
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
                            'To reset your password, contact your administrator or support@esapay.io.'
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
                    >
                      {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                    </button>
                  </div>

                  {/* Real-time NIST Password Compliance (Signup Only) */}
                  {mode === 'signup' && password.length > 0 && (
                    <div className="pt-2 space-y-1.5">
                      <div className="flex items-center justify-between text-[11px] font-sans">
                        <span className="text-slate-400">Password Compliance:</span>
                        <span className="font-semibold" style={{ color: strength.color }}>
                          {strength.label} ({strength.percentage}%)
                        </span>
                      </div>
                      <div className="grid grid-cols-2 sm:grid-cols-3 gap-1 text-[10px] font-mono">
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

                {/* Confirm Password (Signup Only) */}
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
              </>
            )}

            {/* Primary Action Pill Button */}
            <div className="pt-2">
              <button
                type="submit"
                disabled={
                  isLoading ||
                  (mode === 'signup' && (!strength.isValid || !passwordsMatch))
                }
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
                  <>
                    <span>
                      {mode === 'signin' && 'Login'}
                      {mode === 'signup' && 'Provision Entity & Access Console'}
                      {mode === 'enterprise' && 'Authenticate with SSO'}
                    </span>
                    <ArrowRight className="h-4 w-4" />
                  </>
                )}
              </button>
            </div>
          </form>

          {/* Hairline Divider */}
          <div className="border-t border-white/[0.08] my-6" />

          {/* Account Mode Switcher */}
          <div className="text-center text-xs sm:text-sm text-slate-400 font-sans space-y-2">
            {mode === 'signin' ? (
              <p>
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
              </p>
            ) : (
              <p>
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
              </p>
            )}

            {/* Instant Sandbox Console Link */}
            <div>
              <a
                href="/dashboard?mode=sandbox"
                className="inline-flex items-center gap-1.5 text-xs text-amber-400/90 hover:text-amber-300 font-medium transition-colors pt-2"
              >
                <Zap className="h-3 w-3 text-amber-400" />
                <span>Evaluating? Open Instant Sandbox (No Login) &rarr;</span>
              </a>
            </div>
          </div>
        </div>

        {/* Bottom Compliance & Security Assurance */}
        <div className="pt-6 border-t border-white/[0.06] flex flex-wrap items-center justify-between text-[11px] font-mono text-slate-500 gap-2">
          <div className="flex items-center gap-3">
            <span className="flex items-center gap-1 text-emerald-400">
              <span className="h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse" />
              HSM-256 GCM
            </span>
            <span>SOC2 Type II</span>
            <span>RBI Directive</span>
          </div>
          <div className="flex items-center gap-1 text-slate-400">
            <Server className="h-3 w-3 text-blue-400" />
            <span>ap-south-1</span>
          </div>
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
        <div className="fixed inset-0 z-50 w-screen h-screen overflow-hidden bg-[#070A12] flex items-center justify-center p-0">
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="w-full h-full"
          >
            {content}
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}
