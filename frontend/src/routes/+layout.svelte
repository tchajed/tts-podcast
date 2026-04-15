<script>
	import '../app.css';
	let { children } = $props();
</script>

<div class="max-w-3xl mx-auto p-4 sm:p-8">
	<header class="mb-4">
		<a href="/" class="text-xl font-bold no-underline text-base-content hover:no-underline">
			Papercast
		</a>
	</header>
	{@render children()}
</div>
