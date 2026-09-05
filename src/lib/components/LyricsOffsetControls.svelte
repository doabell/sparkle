<script lang="ts">
    interface Props {
        value: number;
        onadjust: (delta: number) => void;
        onreset: () => void;
    }

    let { value, onadjust, onreset }: Props = $props();
    let displayValue = $derived(value > 0 ? `+${value}ms` : `${value}ms`);
</script>

<div class="control-cluster" role="group" aria-label="Lyrics timing offset">
    <button
        type="button"
        aria-label="Shift lyrics earlier"
        title="Shift lyrics earlier"
        onclick={() => onadjust(-50)}>−50</button
    >
    <output aria-label="Current lyrics offset">{displayValue}</output>
    <button
        type="button"
        aria-label="Shift lyrics later"
        title="Shift lyrics later"
        onclick={() => onadjust(50)}>+50</button
    >
    <span class="cluster-divider" aria-hidden="true"></span>
    <button type="button" class="reset" onclick={onreset}>Reset</button>
</div>
