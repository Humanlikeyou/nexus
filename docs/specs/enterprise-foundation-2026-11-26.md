# Amara Enterprise: business workspace foundation (Phase 11)

Saved 2026-11-26 from Taisun's prompt in session. This is Phase 11 in `AMARA_PROJECT_HANDOFF.md` §5 (spec 2, "Amara Enterprise"). It builds on the approved data architecture (`docs/specs/data-architecture-2026-11-25.md`, decision 0053), whose permission table (§6) and `business/amara_business.db` store it extends.

## His prompt (verbatim)

> Build the foundation for Amara Enterprise using the approved business-domain architecture.
>
> The repository currently has a Business page, not a dedicated Enterprise page.
>
> Reuse and extend the existing Business page. Do not create a duplicate business dashboard, remove current working components or mix ARM revenue with TaiyoSkin business performance.
>
> Implement the business workspace foundation:
>
> 1. Business workspace identity.
> 2. Multiple-business support.
> 3. Business-specific context and memory.
> 4. Business-specific documents and knowledge.
> 5. Integration registry.
> 6. Connector status and configuration.
> 7. Permission boundaries.
> 8. Synchronization status.
> 9. Business activity history.
> 10. Business action approvals.
>
> TaiyoSkin is the first business workspace.
>
> Its business context should contain:
> - Approved brand identity and voice.
> - Product catalogue.
> - Approved formulations.
> - Ingredient information.
> - Target audiences.
> - Brand positioning.
> - Business objectives.
> - Website structure.
> - Approved business documents.
> - Historical decisions and recommendations.
>
> Distinguish approved material from drafts and unverified information.
>
> Security:
> - Do not expose private keys or service-role credentials to React.
> - Store integration secrets using a secure local credential mechanism.
> - Do not mix business data with personal memory.
> - Every business-owned record must be scoped to its business.
> - Enforce tenant isolation in every query and operation.
>
> Reuse existing Business components where appropriate.
>
> Do not create fake connected integrations.
>
> Display disconnected, connected, authentication-required, error and synchronization states accurately.
>
> Do not implement website publishing or external write operations yet.
>
> Deliver a functioning business workspace foundation and tests.
