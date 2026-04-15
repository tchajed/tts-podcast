<script lang="ts">
	import { onMount, onDestroy } from 'svelte';

	let { title, text, onclose }: { title: string; text: string; onclose: () => void } = $props();

	function onKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') onclose();
	}

	function onBackdropClick(e: MouseEvent) {
		if (e.target === e.currentTarget) onclose();
	}

	function renderText(raw: string): string {
		return raw
			.split('\n')
			.map((line) => {
				const escaped = line
					.replace(/&/g, '&amp;')
					.replace(/</g, '&lt;')
					.replace(/>/g, '&gt;');
				const headerMatch = escaped.match(/^(#{1,3})\s+(.+)$/);
				if (headerMatch) {
					const level = headerMatch[1].length;
					const cls =
						level === 1
							? 'text-xl font-bold mt-6 mb-2'
							: level === 2
								? 'text-lg font-semibold mt-5 mb-2'
								: 'text-base font-semibold mt-4 mb-1';
					return `<div class="${cls}">${headerMatch[2]}</div>`;
				}
				return escaped;
			})
			.join('\n');
	}

	onMount(() => {
		document.body.style.overflow = 'hidden';
	});

	onDestroy(() => {
		document.body.style.overflow = '';
	});
</script>

<svelte:window onkeydown={onKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
	class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4 sm:p-8"
	role="dialog"
	aria-modal="true"
	aria-label={title}
	tabindex="-1"
	onclick={onBackdropClick}
>
	<div class="bg-base-100 rounded-xl w-full max-w-3xl max-h-[80vh] flex flex-col shadow-xl">
		<div class="flex justify-between items-center px-5 py-3 border-b border-base-300 shrink-0">
			<h2 class="text-lg font-semibold">{title}</h2>
			<button class="btn btn-ghost btn-sm text-xl" onclick={onclose}>&times;</button>
		</div>
		<div class="px-5 py-4 overflow-y-auto whitespace-pre-wrap text-sm leading-7">
			{@html renderText(text)}
		</div>
	</div>
</div>
