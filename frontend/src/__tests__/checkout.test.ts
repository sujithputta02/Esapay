import { describe, it, expect, mock } from 'bun:test';
import {
  executeProviderCheckout,
  submitPayUHostedForm,
  type CheckoutDecision,
} from '../lib/checkout';
import type { ApiClient } from '../lib/api';

describe('Frontend Checkout Execution & State Outcomes', () => {
  describe('Cashfree Provider Flow', () => {
    it('returns captured outcome when backend confirms Cashfree payment as captured', async () => {
      const mockClient = {
        confirmCashfreePayment: mock(async (orderId: string) => ({
          status: 'confirmed',
          payment_state: 'captured',
          order_id: orderId,
          amount: 50000,
          currency: 'INR',
          cf_payment_id: 'cf_pay_99999',
        })),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_cf_123',
        amount: 50000,
        currency: 'INR',
        requested_gateway: 'cashfree',
        routed_gateway: 'cashfree',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://sandbox.cashfree.com/pay',
        provider_order_id: 'order_cf_123',
        session: {
          mode: 'cashfree',
          data: {
            payment_session_id: 'session_cf_tok_123',
            order_id: 'order_cf_123',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient });

      expect(result.status).toBe('captured');
      expect(result.transactionId).toBe('tx_cf_123');
      expect(result.providerOrderId).toBe('order_cf_123');
      expect(result.paymentReference).toBe('cf_pay_99999');
      expect(mockClient.confirmCashfreePayment).toHaveBeenCalledWith('order_cf_123');
    });

    it('returns pending outcome when backend reports Cashfree payment is still pending', async () => {
      const mockClient = {
        confirmCashfreePayment: mock(async () => ({
          status: 'pending',
          payment_state: 'pending',
          order_id: 'order_cf_pending',
        })),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_cf_pend',
        amount: 50000,
        currency: 'INR',
        requested_gateway: 'cashfree',
        routed_gateway: 'cashfree',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://sandbox.cashfree.com/pay',
        provider_order_id: 'order_cf_pending',
        session: {
          mode: 'cashfree',
          data: {
            payment_session_id: 'session_cf_tok_pend',
            order_id: 'order_cf_pending',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient });

      expect(result.status).toBe('pending');
      expect(result.message).toContain('pending');
    });

    it('returns cancelled outcome when Cashfree reports user cancelled/dropped', async () => {
      const mockClient = {
        confirmCashfreePayment: mock(async () => ({
          status: 'cancelled',
          payment_state: 'user_dropped',
          order_id: 'order_cf_drop',
        })),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_cf_drop',
        amount: 50000,
        currency: 'INR',
        requested_gateway: 'cashfree',
        routed_gateway: 'cashfree',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://sandbox.cashfree.com/pay',
        provider_order_id: 'order_cf_drop',
        session: {
          mode: 'cashfree',
          data: {
            payment_session_id: 'session_cf_drop',
            order_id: 'order_cf_drop',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient });

      expect(result.status).toBe('cancelled');
      expect(result.message).toContain('cancelled');
    });

    it('returns failed outcome and NEVER simulates success when Cashfree verification throws an error', async () => {
      const mockClient = {
        confirmCashfreePayment: mock(async () => {
          throw new Error('500 Internal Gateway Error from Cashfree API');
        }),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_cf_err',
        amount: 50000,
        currency: 'INR',
        requested_gateway: 'cashfree',
        routed_gateway: 'cashfree',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://sandbox.cashfree.com/pay',
        provider_order_id: 'order_cf_err',
        session: {
          mode: 'cashfree',
          data: {
            payment_session_id: 'session_cf_err',
            order_id: 'order_cf_err',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient });

      expect(result.status).toBe('failed');
      expect(result.message).toContain('500 Internal Gateway Error');
    });

    it('returns unknown outcome when Cashfree verification times out or network drops', async () => {
      const mockClient = {
        confirmCashfreePayment: mock(async () => {
          throw new Error('Cashfree upstream connection timed out after 10000ms');
        }),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_cf_timeout',
        amount: 50000,
        currency: 'INR',
        requested_gateway: 'cashfree',
        routed_gateway: 'cashfree',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://sandbox.cashfree.com/pay',
        provider_order_id: 'order_cf_timeout',
        session: {
          mode: 'cashfree',
          data: {
            payment_session_id: 'session_cf_timeout',
            order_id: 'order_cf_timeout',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient });

      expect(result.status).toBe('unknown');
      expect(result.message).toContain('uncertain due to network/server timeout');
    });
  });

  describe('PayU Provider Flow', () => {
    it('returns captured outcome when PayU verify payment confirms captured status', async () => {
      const mockClient = {
        verifyPayUPayment: mock(async (txnid: string) => ({
          status: 'verified',
          payment_state: 'captured',
          txnid,
          mihpayid: 'mih_payu_8888',
          amount: 25000,
          currency: 'INR',
        })),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_payu_001',
        amount: 25000,
        currency: 'INR',
        requested_gateway: 'payu',
        routed_gateway: 'payu',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://test.payu.in/_payment',
        provider_order_id: 'payu_order_001',
        session: {
          mode: 'payu',
          data: {
            action_url: 'https://test.payu.in/_payment',
            txnid: 'payu_order_001',
            params: { key: 'test_key', hash: 'abc', txnid: 'payu_order_001' },
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient, skipFormSubmit: true });

      expect(result.status).toBe('captured');
      expect(result.providerOrderId).toBe('payu_order_001');
      expect(result.paymentReference).toBe('mih_payu_8888');
      expect(mockClient.verifyPayUPayment).toHaveBeenCalledWith('payu_order_001');
    });

    it('returns pending outcome when PayU verification is still in progress', async () => {
      const mockClient = {
        verifyPayUPayment: mock(async () => ({
          status: 'pending',
          payment_state: 'pending',
          txnid: 'payu_order_pend',
        })),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_payu_pend',
        amount: 25000,
        currency: 'INR',
        requested_gateway: 'payu',
        routed_gateway: 'payu',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://test.payu.in/_payment',
        provider_order_id: 'payu_order_pend',
        session: {
          mode: 'payu',
          data: {
            action_url: 'https://test.payu.in/_payment',
            txnid: 'payu_order_pend',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient, skipFormSubmit: true });

      expect(result.status).toBe('pending');
    });

    it('returns cancelled outcome when customer cancels on PayU', async () => {
      const mockClient = {
        verifyPayUPayment: mock(async () => ({
          status: 'usercancelled',
          payment_state: 'cancelled',
          txnid: 'payu_order_cancel',
        })),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_payu_cancel',
        amount: 25000,
        currency: 'INR',
        requested_gateway: 'payu',
        routed_gateway: 'payu',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://test.payu.in/_payment',
        provider_order_id: 'payu_order_cancel',
        session: {
          mode: 'payu',
          data: {
            action_url: 'https://test.payu.in/_payment',
            txnid: 'payu_order_cancel',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient, skipFormSubmit: true });

      expect(result.status).toBe('cancelled');
    });

    it('returns failed outcome and NEVER simulates success when PayU verification fails', async () => {
      const mockClient = {
        verifyPayUPayment: mock(async () => {
          throw new Error('Hash mismatch: PayU signature rejected');
        }),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_payu_fail',
        amount: 25000,
        currency: 'INR',
        requested_gateway: 'payu',
        routed_gateway: 'payu',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://test.payu.in/_payment',
        provider_order_id: 'payu_order_fail',
        session: {
          mode: 'payu',
          data: {
            action_url: 'https://test.payu.in/_payment',
            txnid: 'payu_order_fail',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient, skipFormSubmit: true });

      expect(result.status).toBe('failed');
      expect(result.message).toContain('Hash mismatch');
    });

    it('returns unknown outcome when PayU verification times out', async () => {
      const mockClient = {
        verifyPayUPayment: mock(async () => {
          throw new Error('PayU server timeout during verification');
        }),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_payu_timeout',
        amount: 25000,
        currency: 'INR',
        requested_gateway: 'payu',
        routed_gateway: 'payu',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://test.payu.in/_payment',
        provider_order_id: 'payu_order_timeout',
        session: {
          mode: 'payu',
          data: {
            action_url: 'https://test.payu.in/_payment',
            txnid: 'payu_order_timeout',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient, skipFormSubmit: true });

      expect(result.status).toBe('unknown');
      expect(result.message).toContain('uncertain due to network/server timeout');
    });

    it('throws error when DOM document is not available', () => {
      expect(() =>
        submitPayUHostedForm('https://test.payu.in/_payment', { key: '123' })
      ).toThrow('DOM document not available');
    });

    it('creates and submits an HTML form with hidden fields when document is present', () => {
      const submitMock = mock(() => {});
      const appendMock = mock(() => {});
      const formMock = {
        method: '',
        action: '',
        target: '',
        style: {} as any,
        appendChild: appendMock,
        submit: submitMock,
      };

      (globalThis as any).document = {
        createElement: mock((tag: string) => {
          if (tag === 'form') return formMock;
          return { type: '', name: '', value: '' };
        }),
        body: {
          appendChild: mock(() => {}),
        },
      };

      const form = submitPayUHostedForm('https://test.payu.in/_payment', {
        key: 'test_key',
        txnid: 'tx_123',
        amount: '100.00',
      });

      expect(form.method).toBe('POST');
      expect(form.action).toBe('https://test.payu.in/_payment');
      expect(submitMock).toHaveBeenCalled();

      delete (globalThis as any).document;
    });
  });

  describe('Razorpay Provider Flow', () => {
    it('returns captured outcome when Razorpay confirmation endpoint verifies captured state', async () => {
      const mockClient = {
        confirmRazorpayPayment: mock(async (paymentId: string) => ({
          status: 'confirmed',
          payment_state: 'captured',
          payment_id: paymentId,
        })),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_rzp_001',
        amount: 10000,
        currency: 'INR',
        requested_gateway: 'razorpay',
        routed_gateway: 'razorpay',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://api.razorpay.com/checkout',
        provider_order_id: 'order_rzp_001',
        session: {
          mode: 'razorpay',
          data: {
            order_id: 'order_rzp_001',
            key_id: 'rzp_test_key',
            payment_id: 'pay_rzp_verified_123',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient });

      expect(result.status).toBe('captured');
      expect(result.paymentReference).toBe('pay_rzp_verified_123');
      expect(mockClient.confirmRazorpayPayment).toHaveBeenCalledWith('pay_rzp_verified_123');
    });

    it('returns failed outcome when Razorpay confirmation fails', async () => {
      const mockClient = {
        confirmRazorpayPayment: mock(async () => {
          throw new Error('403 Forbidden: merchant ownership violation');
        }),
      } as unknown as ApiClient;

      const decision: CheckoutDecision = {
        transaction_id: 'tx_rzp_fail',
        amount: 10000,
        currency: 'INR',
        requested_gateway: 'razorpay',
        routed_gateway: 'razorpay',
        failover_triggered: false,
        routing_reason: 'Direct route',
        checkout_url: 'https://api.razorpay.com/checkout',
        provider_order_id: 'order_rzp_fail',
        session: {
          mode: 'razorpay',
          data: {
            order_id: 'order_rzp_fail',
            payment_id: 'pay_rzp_bad',
          },
        },
      };

      const result = await executeProviderCheckout(decision, { client: mockClient });

      expect(result.status).toBe('failed');
      expect(result.message).toContain('403 Forbidden');
    });
  });

  describe('Invalid or Edge Case Sessions', () => {
    it('returns failed when transaction_id is missing', async () => {
      const result = await executeProviderCheckout({} as any);
      expect(result.status).toBe('failed');
    });

    it('returns failed when session is missing', async () => {
      const result = await executeProviderCheckout({
        transaction_id: 'tx_nosession',
        amount: 1000,
        currency: 'INR',
        requested_gateway: 'cashfree',
        routed_gateway: 'cashfree',
        failover_triggered: false,
        routing_reason: 'Direct',
        checkout_url: '',
      });
      expect(result.status).toBe('failed');
      expect(result.message).toContain('No active provider session');
    });

    it('returns unknown when session mode is unrecognized', async () => {
      const result = await executeProviderCheckout({
        transaction_id: 'tx_unknown',
        amount: 1000,
        currency: 'INR',
        requested_gateway: 'future_gw',
        routed_gateway: 'future_gw',
        failover_triggered: false,
        routing_reason: 'Direct',
        checkout_url: '',
        session: {
          mode: 'unsupported_provider' as any,
          data: {},
        },
      });
      expect(result.status).toBe('unknown');
    });
  });
});

