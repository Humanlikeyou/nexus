# Website operations workflow (Phase 13)

Saved 2026-11-26 from Taisun's prompt in session. Phase 13 in `AMARA_PROJECT_HANDOFF.md` §5. Builds on 0063 (workspaces) and 0064 (read-only connectors).

## His prompt (verbatim)

> Extend Amara Enterprise with a controlled website operations workflow.
>
> Do not assume Hostinger provides an API for every requested operation. First inspect the actual deployment method and identify the supported way to manage the TaiyoSkin website.
>
> Implement a staged workflow:
>
> 1. Locate the requested product or content.
> 2. Retrieve the latest approved business information.
> 3. Identify the current website content.
> 4. Generate a proposed revision.
> 5. Show a before-and-after comparison.
> 6. Validate relevant links, formatting and content.
> 7. Create a preview.
> 8. Obtain explicit user approval.
> 9. Back up the current version.
> 10. Publish the approved change.
> 11. Verify the deployed result.
> 12. Record the result in the audit history.
> 13. Provide rollback if verification fails.
>
> Support content types as they become available:
> - Product pages.
> - Product descriptions.
> - Ingredient information.
> - FAQs.
> - Blog content.
> - Landing pages.
> - SEO metadata.
> - Images and website assets.
>
> Permission requirements:
>
> - Drafting requires no publishing permission.
> - Publishing requires explicit approval.
> - Pricing changes require explicit approval.
> - Product formulation changes require explicit approval.
> - Customer communications require explicit approval.
> - Deletion requires explicit approval.
>
> Maintain version history and rollback capability.
>
> Never overwrite website files silently.
>
> Never substitute unapproved formulation information or outdated documents for approved product information.
>
> Test the workflow with a non-public preview or safe test target before using the production website.
