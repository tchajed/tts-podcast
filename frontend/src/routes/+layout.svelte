<script>
	import '../app.css';
	let { children } = $props();
</script>

<div class="container">
	<header class="mb-2">
		<a href="/" style="text-decoration: none; color: inherit;">
			<h1>TTS Podcast</h1>
		</a>
	</header>
	{@render children()}
</div>
