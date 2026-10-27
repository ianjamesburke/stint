---
id: "0006"
title: "Stripe subscription webhook handler"
status: backlog
priority: p2
size: l
estimate: "4h"
sprint: "s2"
area:
  - "billing"
tags:
  - "payments"
---

Handle incoming `customer.subscription.updated` and `invoice.payment_succeeded` webhooks from Stripe.

## Why

Required for automated recurring subscription upgrades and access provisioning. Planned for Sprint 2.

## Done When

- [ ] Webhook endpoint verifies Stripe signature header with webhook signing secret
- [ ] Subscription status changes update customer subscription records in database
- [ ] Idempotency key tracking prevents duplicate invoice processing
