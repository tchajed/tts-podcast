<script lang="ts">
	let { message, onclose }: { message: string; onclose: () => void } = $props();

	$effect(() => {
		const timer = setTimeout(onclose, 2000);
		return () => clearTimeout(timer);
	});
</script>

<div class="toast toast-center toast-bottom z-50">
	<div class="alert alert-info">
		<span>{message}</span>
	</div>
</div>
