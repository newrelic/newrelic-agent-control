Goals:
- Easily navigate through workflows when modifying them.
- Easily identify them from the UI whenever manual triggering is needed.

Icon and naming standards:
- For `workflow_dispatch`(generally triggered manually from the UI):
  - file names prefixed with `on_demand_`.
  - name prefixed with icon: ⏯️
- For `workflow_call`(another workflow):
  - file names prefixed with `component_`.
  - name prefixed with icon: 📞

Whenever GH starts supporting other way to organize workflows, we can update this.
