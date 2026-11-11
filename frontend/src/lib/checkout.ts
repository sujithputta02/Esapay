import { apiClient, ApiClient } from './api';

export type PaymentOutcomeStatus =
  | 'idle'
  | 'processing'
  | 'pending'
  | 'captured'
  | 'failed'
  | 'cancelled'
  | 'unknown';

export interface CheckoutDecision {
  transaction_id: string;
  amount: number;
  amount_minor?: number;
  currency: string;
  requested_gateway: string;
  routed_gateway: string;
  failover_triggered: boolean;
  routing_reason: string;
  checkout_url: string;
  is_simulated?: boolean;
  session?: {
    mode: 'razorpay' | 'cashfree' | 'payu' | 'hosted_url';
    data: any;
  };
  provider_order_id?: string;
}

export interface PaymentExecutionResult {
  status: PaymentOutcomeStatus;
  message?: string;
  transactionId?: string;
  providerOrderId?: string;
  paymentReference?: string;
  details?: Record<string, any>;
}

export interface CheckoutExecutionOptions {
  client?: ApiClient;
  skipFormSubmit?: boolean; // For testing or headless verification
}

/**
 * Submits PayU Hosted Checkout form securely by constructing and posting a form to the gateway URL.
 */
export function submitPayUHostedForm(
  actionUrl: string,
  params: Record<string, string>,
  target: string = '_self'
): HTMLFormElement {
  if (typeof document === 'undefined') {
    throw new Error('DOM document not available for form submission');
  }
  const form = document.createElement('form');
  form.method = 'POST';
  form.action = actionUrl;
  form.target = target;
  form.style.display = 'none';

  for (const [key, value] of Object.entries(params)) {
    const input = document.createElement('input');
    input.type = 'hidden';
    input.name = key;
    input.value = value !== undefined && value !== null ? String(value) : '';
    form.appendChild(input);
  }

  document.body.appendChild(form);
  form.submit();
  return form;
}

/**
 * Executes checkout against the routed payment gateway and authoritatively verifies
 * payment status with the backend before concluding success.
 *
 * Never treats a verification failure, SDK error, or unverified response as simulated success.
 */
function isUncertainError(err: any): boolean {
  if (!err) return false;
  const msg = (err.message || '').toLowerCase();
  return (
    err.name === 'AbortError' ||
    msg.includes('timeout') ||
    msg.includes('timed out') ||
    msg.includes('network') ||
    msg.includes('connection') ||
    err.status === 504 ||
    err.status === 502 ||
    err.status === 503
  );
}

export async function executeProviderCheckout(
  decision: CheckoutDecision,
  options: CheckoutExecutionOptions = {}
): Promise<PaymentExecutionResult> {
  const client = options.client || apiClient;

  if (!decision || !decision.transaction_id) {
    return {
      status: 'failed',
      message: 'Invalid checkout decision payload',
    };
  }

  const session = decision.session;
  if (!session) {
    return {
      status: 'failed',
      message: 'No active provider session returned from router',
      transactionId: decision.transaction_id,
    };
  }

  try {
    switch (session.mode) {
      case 'cashfree': {
        const paymentSessionId = session.data?.payment_session_id;
        const orderId = decision.provider_order_id || session.data?.order_id;

        if (!orderId) {
          return {
            status: 'failed',
            message: 'Missing order_id for Cashfree checkout confirmation',
            transactionId: decision.transaction_id,
          };
        }

        // 1. Invoke Cashfree Web SDK if available in the browser window
        if (typeof window !== 'undefined' && (window as any).Cashfree && paymentSessionId) {
          try {
            const cashfreeFactory = (window as any).Cashfree;
            const mode = session.data?.environment === 'production' ? 'production' : 'sandbox';
            const cashfree =
              typeof cashfreeFactory === 'function'
                ? new cashfreeFactory({ mode })
                : cashfreeFactory;

            if (cashfree && typeof cashfree.checkout === 'function') {
              await cashfree.checkout({
                paymentSessionId,
                redirectTarget: '_modal',
              });
            }
          } catch (err: any) {
            const msg = (err?.message || '').toLowerCase();
            if (msg.includes('cancel') || msg.includes('closed') || msg.includes('dismiss')) {
              return {
                status: 'cancelled',
                message: 'Cashfree checkout was cancelled by the user',
                transactionId: decision.transaction_id,
                providerOrderId: orderId,
              };
            }
            // SDK error - do not fake success
            return {
              status: 'failed',
              message: `Cashfree SDK error: ${err?.message || 'Checkout failed'}`,
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
            };
          }
        }

        // 2. Authoritative backend verification
        try {
          const verification = await client.confirmCashfreePayment(orderId);
          const state = (verification.payment_state || verification.status || '').toLowerCase();

          if (state === 'captured' || state === 'confirmed' || state === 'paid') {
            return {
              status: 'captured',
              message: 'Payment captured and verified authoritatively with Cashfree.',
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
              paymentReference: verification.cf_payment_id,
              details: verification,
            };
          } else if (state === 'pending' || state === 'active') {
            return {
              status: 'pending',
              message: 'Payment authorization is pending confirmation with Cashfree.',
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
              details: verification,
            };
          } else if (state === 'user_dropped' || state === 'cancelled') {
            return {
              status: 'cancelled',
              message: 'Payment cancelled or abandoned by customer on Cashfree.',
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
              details: verification,
            };
          } else {
            return {
              status: 'failed',
              message: `Cashfree payment failed (status: ${verification.payment_state || verification.status})`,
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
              details: verification,
            };
          }
        } catch (err: any) {
          if (isUncertainError(err)) {
            return {
              status: 'unknown',
              message: `Cashfree verification outcome uncertain due to network/server timeout. Do not re-attempt payment before verifying: ${err?.message || 'Connection timeout'}`,
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
            };
          }
          return {
            status: 'failed',
            message: `Cashfree verification failed: ${err?.message || 'Verification rejected'}`,
            transactionId: decision.transaction_id,
            providerOrderId: orderId,
          };
        }
      }

      case 'payu': {
        const txnid = decision.provider_order_id || session.data?.txnid || decision.transaction_id;
        const actionUrl = session.data?.action_url;
        const params = session.data?.params;

        if (actionUrl && params && typeof document !== 'undefined' && !options.skipFormSubmit) {
          // If live form submission is enabled in the browser:
          try {
            submitPayUHostedForm(actionUrl, params);
            return {
              status: 'processing',
              message: 'Redirecting to PayU Hosted Checkout...',
              transactionId: decision.transaction_id,
              providerOrderId: txnid,
            };
          } catch (err: any) {
            return {
              status: 'failed',
              message: `Failed to submit PayU hosted checkout form: ${err?.message}`,
              transactionId: decision.transaction_id,
              providerOrderId: txnid,
            };
          }
        }

        // Authoritative verification via PayU verify endpoint
        try {
          const verification = await client.verifyPayUPayment(txnid);
          const state = (verification.payment_state || verification.status || '').toLowerCase();

          if (state === 'captured' || state === 'verified' || state === 'success') {
            return {
              status: 'captured',
              message: 'PayU transaction verified and captured authoritatively.',
              transactionId: decision.transaction_id,
              providerOrderId: txnid,
              paymentReference: verification.mihpayid,
              details: verification,
            };
          } else if (state === 'pending' || state === 'in_progress') {
            return {
              status: 'pending',
              message: 'PayU payment is pending verification.',
              transactionId: decision.transaction_id,
              providerOrderId: txnid,
              details: verification,
            };
          } else if (state === 'cancelled' || state === 'usercancelled') {
            return {
              status: 'cancelled',
              message: 'PayU checkout was cancelled.',
              transactionId: decision.transaction_id,
              providerOrderId: txnid,
              details: verification,
            };
          } else {
            return {
              status: 'failed',
              message: `PayU payment verification returned unsuccessful status: ${verification.status || verification.payment_state}`,
              transactionId: decision.transaction_id,
              providerOrderId: txnid,
              details: verification,
            };
          }
        } catch (err: any) {
          if (isUncertainError(err)) {
            return {
              status: 'unknown',
              message: `PayU verification outcome uncertain due to network/server timeout. Do not re-attempt payment before verifying: ${err?.message || 'Connection timeout'}`,
              transactionId: decision.transaction_id,
              providerOrderId: txnid,
            };
          }
          return {
            status: 'failed',
            message: `PayU verification failed: ${err?.message || 'Verification rejected'}`,
            transactionId: decision.transaction_id,
            providerOrderId: txnid,
          };
        }
      }

      case 'razorpay': {
        const orderId = decision.provider_order_id || session.data?.order_id;
        const keyId = session.data?.key_id;

        // In browser context with Razorpay SDK:
        if (typeof window !== 'undefined' && (window as any).Razorpay && orderId && keyId) {
          return new Promise<PaymentExecutionResult>((resolve) => {
            try {
              const rzp = new (window as any).Razorpay({
                key: keyId,
                order_id: orderId,
                amount: session.data?.amount,
                currency: session.data?.currency || 'INR',
                name: 'ESAPay Checkout',
                description: 'Autonomous Universal Payment Gateway',
                handler: async (response: any) => {
                  const paymentId = response?.razorpay_payment_id;
                  if (!paymentId) {
                    resolve({
                      status: 'failed',
                      message: 'Razorpay returned empty payment_id',
                      transactionId: decision.transaction_id,
                      providerOrderId: orderId,
                    });
                    return;
                  }
                  try {
                    const confirmRes = await client.confirmRazorpayPayment(paymentId);
                    const pState = (confirmRes.payment_state || confirmRes.status || '').toLowerCase();
                    if (pState === 'captured' || pState === 'confirmed' || pState === 'already_confirmed') {
                      resolve({
                        status: 'captured',
                        message: 'Razorpay payment authoritatively captured.',
                        transactionId: decision.transaction_id,
                        providerOrderId: orderId,
                        paymentReference: paymentId,
                        details: confirmRes,
                      });
                    } else {
                      resolve({
                        status: 'failed',
                        message: `Razorpay confirmation returned non-captured state: ${pState}`,
                        transactionId: decision.transaction_id,
                        providerOrderId: orderId,
                        paymentReference: paymentId,
                        details: confirmRes,
                      });
                    }
                  } catch (confErr: any) {
                    if (isUncertainError(confErr)) {
                      resolve({
                        status: 'unknown',
                        message: `Razorpay confirmation outcome uncertain due to network/server timeout. Do not re-attempt payment before verifying: ${confErr?.message || 'Connection timeout'}`,
                        transactionId: decision.transaction_id,
                        providerOrderId: orderId,
                        paymentReference: paymentId,
                      });
                    } else {
                      resolve({
                        status: 'failed',
                        message: `Razorpay backend confirmation failed: ${confErr?.message || 'Rejected'}`,
                        transactionId: decision.transaction_id,
                        providerOrderId: orderId,
                      });
                    }
                  }
                },
                modal: {
                  ondismiss: () => {
                    resolve({
                      status: 'cancelled',
                      message: 'Razorpay checkout dismissed by user',
                      transactionId: decision.transaction_id,
                      providerOrderId: orderId,
                    });
                  },
                },
              });
              rzp.open();
            } catch (rzpErr: any) {
              resolve({
                status: 'failed',
                message: `Razorpay SDK initialization failed: ${rzpErr?.message}`,
                transactionId: decision.transaction_id,
                providerOrderId: orderId,
              });
            }
          });
        }

        // If SDK is not present or running in test/verification mode:
        if (session.data?.payment_id) {
          try {
            const confirmRes = await client.confirmRazorpayPayment(session.data.payment_id);
            const pState = (confirmRes.payment_state || confirmRes.status || '').toLowerCase();
            if (pState === 'captured' || pState === 'confirmed' || pState === 'already_confirmed') {
              return {
                status: 'captured',
                message: 'Razorpay payment authoritatively captured.',
                transactionId: decision.transaction_id,
                providerOrderId: orderId,
                paymentReference: session.data.payment_id,
                details: confirmRes,
              };
            }
            return {
              status: 'failed',
              message: `Razorpay confirmation returned state: ${pState}`,
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
              details: confirmRes,
            };
          } catch (err: any) {
            return {
              status: 'failed',
              message: `Razorpay confirmation error: ${err?.message}`,
              transactionId: decision.transaction_id,
              providerOrderId: orderId,
            };
          }
        }

        return {
          status: 'pending',
          message: 'Razorpay order created; awaiting customer completion.',
          transactionId: decision.transaction_id,
          providerOrderId: orderId,
        };
      }

      case 'hosted_url': {
        if (decision.checkout_url && typeof window !== 'undefined' && !options.skipFormSubmit) {
          window.location.href = decision.checkout_url;
          return {
            status: 'processing',
            message: 'Redirecting to hosted checkout...',
            transactionId: decision.transaction_id,
          };
        }
        return {
          status: 'pending',
          message: 'Hosted checkout URL generated; awaiting completion.',
          transactionId: decision.transaction_id,
        };
      }

      default:
        return {
          status: 'unknown',
          message: `Unrecognized checkout session mode: ${(session as any).mode}`,
          transactionId: decision.transaction_id,
        };
    }
  } catch (err: any) {
    return {
      status: 'failed',
      message: `Checkout execution encountered unhandled error: ${err?.message || 'Unknown failure'}`,
      transactionId: decision.transaction_id,
    };
  }
}

