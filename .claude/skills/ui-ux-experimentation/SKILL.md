---
name: UI/UX Experimentation
description: Use when experimenting with different user interfaces or user experiences.
---

<required>
*CRITICAL* Add the following steps to your Todo list using TodoWrite:

1. Ask me questions to understand the following:
- What do I want to see design variants of?
- How many variants do I want to see?

Brainstorm with me on different kinds of variants. Ask me questions about what my goal is. Constantly propose suggestions and comparisons.

2. If building a web frontend, start the dev server and note the localhost URL. For non-web frontends, identify how to preview the UI.
3. Implement the UI/UX design(s)
4. Ask me for feedback on the design
5. Go back to step 3 and iterate based on feedback.
6. Read and follow `/home/amol/code/nori/nori-cli/.worktrees/vast-pen-20260309-211245/.claude/skills/building-user-interface-elements/SKILL.md` to properly take the finished design and implement it.
</required>

# Further guidance

## Implementing multiple variations

Stack variations in a way that makes comparison easy. For web UIs, this typically means:
- Render all variations on a single page, stacked vertically
- Add clear section dividers/headings for each variation
- Use consistent spacing between variations
- Ensure each variation is self-contained and functional

**Example for React:**
```tsx
export default function UIExploration() {
  return (
    <div className="ui-exploration">
      <section className="variation">
        <h2>Variation 1: Minimalist</h2>
        <MinimalistLogin />
      </section>

      <section className="variation">
        <h2>Variation 2: Modern Gradient</h2>
        <ModernLogin />
      </section>

      <section className="variation">
        <h2>Variation 3: Classic Corporate</h2>
        <ClassicLogin />
      </section>
    </div>
  );
}
```

When implementing a single design:
- Focus on clean, production-ready implementation
- Follow project conventions and style guides
- Ensure responsive design if applicable

## Feedback & Iteration

Present to user:
- Share the localhost URL or preview mechanism
- Briefly describe each variation (if multiple)
- Ask questions about things that you were uncertain about.
- Propose follow up directions.

Iterate based on feedback:
- Make requested changes
- Continue asking for feedback until satisfied

## Final Integration & Wiring

Ensure proper integration:

- Remove exploration scaffolding (if used)
- Wire up to actual data sources/APIs
- Connect to routing/navigation
- Add proper state management
- Implement error handling
- Add loading states if applicable
- Test all interactive elements
- Verify accessibility basics

Common integration points to check:
- Form submissions → backend endpoints
- Navigation → routing system
- Authentication → auth context/store
- Data fetching → API layer
- Error boundaries → error handling
- Responsive behavior → breakpoints
