<script lang="ts">
    interface Props {
        value: number;
        onadjust: (delta: number) => void;
        onreset: () => void;
        disabled?: boolean;
    }

    let { value, onadjust, onreset, disabled = false }: Props = $props();
    let displayValue = $derived(value > 0 ? `+${value}ms` : `${value}ms`);
</script>

<div class="control-cluster" role="group" aria-label="Lyrics timing offset">
    <button
        type="button"
        {disabled}
        aria-label="Shift lyrics 100 ms earlier"
        title="Shift lyrics 100 ms earlier"
        onclick={() => onadjust(-100)}>−100</button
    >
    <button
        type="button"
        {disabled}
        aria-label="Shift lyrics 50 ms earlier"
        title="Shift lyrics 50 ms earlier"
        onclick={() => onadjust(-50)}>−50</button
    >
    <output aria-label="Current lyrics offset">{displayValue}</output>
    <button
        type="button"
        {disabled}
        aria-label="Shift lyrics 50 ms later"
        title="Shift lyrics 50 ms later"
        onclick={() => onadjust(50)}>+50</button
    >
    <button
        type="button"
        {disabled}
        aria-label="Shift lyrics 100 ms later"
        title="Shift lyrics 100 ms later"
        onclick={() => onadjust(100)}>+100</button
    >
    <span class="cluster-divider" aria-hidden="true"></span>
    <button type="button" class="reset" {disabled} onclick={onreset}
        >Reset</button
    >
</div>
