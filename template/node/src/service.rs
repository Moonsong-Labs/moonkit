//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

// std
use std::{sync::Arc, time::Duration};

// Local Runtime Types
use moonkit_template_runtime::{
	opaque::{Block, Hash},
	RuntimeApi,
};

use nimbus_consensus::NimbusManualSealConsensusDataProvider;

// Cumulus Imports
use cumulus_client_cli::CollatorOptions;
use cumulus_client_collator::service::CollatorService;
use cumulus_client_consensus_common::ParachainBlockImport as TParachainBlockImport;
use cumulus_client_consensus_proposer::Proposer;
use cumulus_client_network::RequireSecondedInBlockAnnounce;
use cumulus_client_parachain_inherent::{MockValidationDataInherentDataProvider, MockXcmConfig};
#[allow(deprecated)]
use cumulus_client_service::{
	prepare_node_config, start_relay_chain_tasks, DARecoveryProfile, StartRelayChainTasksParams,
};
use cumulus_primitives_core::CollectCollationInfo;
use cumulus_primitives_core::ParaId;
use cumulus_relay_chain_inprocess_interface::build_inprocess_relay_chain;
use cumulus_relay_chain_interface::{OverseerHandle, RelayChainInterface, RelayChainResult};
use cumulus_relay_chain_minimal_node::build_minimal_relay_chain_node_with_rpc;

use polkadot_primitives::UpgradeGoAhead;
use polkadot_service::CollatorPair;

// Substrate Imports
use futures::FutureExt;
use sc_client_api::Backend;
use sc_consensus::ImportQueue;
use sc_consensus_manual_seal::{run_instant_seal, InstantSealParams};
#[allow(deprecated)]
use sc_executor::{
	HeapAllocStrategy, NativeElseWasmExecutor, WasmExecutor, DEFAULT_HEAP_ALLOC_STRATEGY,
};
use sc_network::{
	config::FullNetworkConfiguration, request_responses::IncomingRequest as GenericIncomingRequest,
	service::traits::NetworkService, NetworkBlock,
};
use sc_service::{Configuration, PartialComponents, TFullBackend, TFullClient, TaskManager};
use sc_telemetry::{Telemetry, TelemetryHandle, TelemetryWorker, TelemetryWorkerHandle};
use sp_api::ProvideRuntimeApi;
use sp_keystore::KeystorePtr;
use sp_runtime::traits::Block as BlockT;
use substrate_prometheus_endpoint::Registry;
/// Native executor instance.
pub struct TemplateRuntimeExecutor;

impl sc_executor::NativeExecutionDispatch for TemplateRuntimeExecutor {
	type ExtendHostFunctions = frame_benchmarking::benchmarking::HostFunctions;

	fn dispatch(method: &str, data: &[u8]) -> Option<Vec<u8>> {
		moonkit_template_runtime::api::dispatch(method, data)
	}

	fn native_version() -> sc_executor::NativeVersion {
		moonkit_template_runtime::native_version()
	}
}
#[allow(deprecated)]
type ParachainExecutor = NativeElseWasmExecutor<TemplateRuntimeExecutor>;

type ParachainClient = TFullClient<Block, RuntimeApi, ParachainExecutor>;

type ParachainBackend = TFullBackend<Block>;

type ParachainBlockImport = TParachainBlockImport<Block, Arc<ParachainClient>, ParachainBackend>;

/// Starts a `ServiceBuilder` for a full service.
///
/// Use this macro if you don't actually need the full service, but just the builder in order to
/// be able to perform chain operations.
pub fn new_partial(
	config: &Configuration,
) -> Result<
	PartialComponents<
		ParachainClient,
		ParachainBackend,
		sc_consensus::LongestChain<ParachainBackend, Block>,
		sc_consensus::DefaultImportQueue<Block>,
		sc_transaction_pool::TransactionPoolHandle<Block, ParachainClient>,
		(
			ParachainBlockImport,
			Option<Telemetry>,
			Option<TelemetryWorkerHandle>,
		),
	>,
	sc_service::Error,
> {
	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let heap_pages = config
		.executor
		.default_heap_pages
		.map_or(DEFAULT_HEAP_ALLOC_STRATEGY, |h| HeapAllocStrategy::Static {
			extra_pages: h as _,
		});

	let wasm = WasmExecutor::builder()
		.with_execution_method(config.executor.wasm_method)
		.with_onchain_heap_alloc_strategy(heap_pages)
		.with_offchain_heap_alloc_strategy(heap_pages)
		.with_max_runtime_instances(config.executor.max_runtime_instances)
		.with_runtime_cache_size(config.executor.runtime_cache_size)
		.build();

	let executor = ParachainExecutor::new_with_wasm_executor(wasm);

	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts::<Block, RuntimeApi, _>(
			config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
		)?;
	let client = Arc::new(client);

	let telemetry_worker_handle = telemetry.as_ref().map(|(worker, _)| worker.handle());

	let telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager
			.spawn_handle()
			.spawn("telemetry", None, worker.run());
		telemetry
	});

	// Although this will not be used by the parachain collator, it will be used by the instant seal
	// And sovereign nodes, so we create it anyway.
	let select_chain = sc_consensus::LongestChain::new(backend.clone());

	let transaction_pool = sc_transaction_pool::Builder::new(
		task_manager.spawn_essential_handle(),
		client.clone(),
		config.role.is_authority().into(),
	)
	.with_options(config.transaction_pool.clone())
	.with_prometheus(config.prometheus_registry())
	.build();

	let block_import = ParachainBlockImport::new(client.clone(), backend.clone());

	let import_queue = nimbus_consensus::import_queue(
		client.clone(),
		client.clone(),
		move |_, _| async move {
			let time = sp_timestamp::InherentDataProvider::from_system_time();

			Ok((time,))
		},
		&task_manager.spawn_essential_handle(),
		config.prometheus_registry().clone(),
		false,
		false,
	)?;

	Ok(PartialComponents {
		backend,
		client,
		import_queue,
		keystore_container,
		task_manager,
		transaction_pool: transaction_pool.into(),
		select_chain,
		other: (block_import, telemetry, telemetry_worker_handle),
	})
}

async fn build_relay_chain_interface(
	polkadot_config: Configuration,
	parachain_config: &Configuration,
	telemetry_worker_handle: Option<TelemetryWorkerHandle>,
	task_manager: &mut TaskManager,
	collator_options: CollatorOptions,
) -> RelayChainResult<(
	Arc<(dyn RelayChainInterface + 'static)>,
	Option<CollatorPair>,
	Arc<dyn NetworkService>,
	async_channel::Receiver<GenericIncomingRequest>,
)> {
	if let cumulus_client_cli::RelayChainMode::ExternalRpc(rpc_target_urls) =
		collator_options.relay_chain_mode
	{
		build_minimal_relay_chain_node_with_rpc(
			polkadot_config,
			parachain_config.prometheus_registry(),
			task_manager,
			rpc_target_urls,
		)
		.await
	} else {
		build_inprocess_relay_chain(
			polkadot_config,
			parachain_config,
			telemetry_worker_handle,
			task_manager,
			None,
		)
	}
}

/// Start a node with the given parachain `Configuration` and relay chain `Configuration`.
///
/// This is the actual implementation that is abstract over the executor and the runtime api.
#[sc_tracing::logging::prefix_logs_with("Parachain")]
async fn start_node_impl<N>(
	parachain_config: Configuration,
	polkadot_config: Configuration,
	collator_options: CollatorOptions,
	para_id: ParaId,
	max_pov_percentage: u8,
) -> sc_service::error::Result<(TaskManager, Arc<ParachainClient>)>
where
	N: sc_network::NetworkBackend<Block, <Block as BlockT>::Hash>,
{
	let parachain_config = prepare_node_config(parachain_config);

	let params = new_partial(&parachain_config)?;
	let (block_import, mut telemetry, telemetry_worker_handle) = params.other;

	let client = params.client.clone();
	let backend = params.backend.clone();
	let mut task_manager = params.task_manager;

	let (relay_chain_interface, collator_key, _, _) = build_relay_chain_interface(
		polkadot_config,
		&parachain_config,
		telemetry_worker_handle,
		&mut task_manager,
		collator_options.clone(),
	)
	.await
	.map_err(|e| sc_service::Error::Application(Box::new(e) as Box<_>))?;

	let block_announce_validator =
		RequireSecondedInBlockAnnounce::new(relay_chain_interface.clone(), para_id);

	let validator = parachain_config.role.is_authority();
	let prometheus_registry = parachain_config.prometheus_registry().cloned();
	let transaction_pool = params.transaction_pool.clone();
	let import_queue_service = params.import_queue.service();

	let net_config = FullNetworkConfiguration::<_, _, N>::new(
		&parachain_config.network,
		prometheus_registry.clone(),
	);

	let metrics = N::register_notification_metrics(
		parachain_config
			.prometheus_config
			.as_ref()
			.map(|cfg| &cfg.registry),
	);

	let (network, system_rpc_tx, tx_handler_controller, sync_service) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &parachain_config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			import_queue: params.import_queue,
			block_announce_validator_builder: Some(Box::new(|_| {
				Box::new(block_announce_validator)
			})),
			warp_sync_config: None,
			net_config,
			block_relay: None,
			metrics,
		})?;

	let rpc_extensions_builder = {
		let client = client.clone();
		let transaction_pool = transaction_pool.clone();

		Box::new(move |_| {
			let deps = crate::rpc::FullDeps {
				client: client.clone(),
				pool: transaction_pool.clone(),
			};

			crate::rpc::create_full(deps).map_err(Into::into)
		})
	};

	let force_authoring = parachain_config.force_authoring;

	sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		rpc_builder: rpc_extensions_builder,
		client: client.clone(),
		transaction_pool: transaction_pool.clone(),
		task_manager: &mut task_manager,
		config: parachain_config,
		keystore: params.keystore_container.keystore(),
		backend: backend.clone(),
		network: network.clone(),
		sync_service: sync_service.clone(),
		system_rpc_tx,
		tx_handler_controller,
		telemetry: telemetry.as_mut(),
	})?;

	let announce_block = {
		let sync_service = sync_service.clone();
		Arc::new(move |hash, data| sync_service.announce_block(hash, data))
	};

	let relay_chain_slot_duration = Duration::from_secs(6);
	let overseer_handle = relay_chain_interface
		.overseer_handle()
		.map_err(|e| sc_service::Error::Application(Box::new(e)))?;

	start_relay_chain_tasks(StartRelayChainTasksParams {
		client: client.clone(),
		announce_block: announce_block.clone(),
		para_id,
		relay_chain_interface: relay_chain_interface.clone(),
		task_manager: &mut task_manager,
		da_recovery_profile: if validator {
			DARecoveryProfile::Collator
		} else {
			DARecoveryProfile::FullNode
		},
		import_queue: import_queue_service,
		relay_chain_slot_duration,
		recovery_handle: Box::new(overseer_handle.clone()),
		sync_service: sync_service.clone(),
		prometheus_registry: Default::default(), // TODO
	})?;

	if validator {
		start_consensus(
			client.clone(),
			block_import,
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|t| t.handle()),
			&task_manager,
			relay_chain_interface.clone(),
			transaction_pool,
			params.keystore_container.keystore(),
			para_id,
			collator_key.expect("Command line arguments do not allow this. qed"),
			overseer_handle,
			announce_block,
			force_authoring,
			max_pov_percentage,
		)?;
	}

	Ok((task_manager, client))
}

fn start_consensus(
	client: Arc<ParachainClient>,
	block_import: ParachainBlockImport,
	prometheus_registry: Option<&Registry>,
	telemetry: Option<TelemetryHandle>,
	task_manager: &TaskManager,
	relay_chain_interface: Arc<dyn RelayChainInterface>,
	transaction_pool: Arc<sc_transaction_pool::TransactionPoolHandle<Block, ParachainClient>>,
	keystore: KeystorePtr,
	para_id: ParaId,
	collator_key: CollatorPair,
	overseer_handle: OverseerHandle,
	announce_block: Arc<dyn Fn(Hash, Option<Vec<u8>>) + Send + Sync>,
	force_authoring: bool,
	max_pov_percentage: u8,
) -> Result<(), sc_service::Error> {
	let proposer_factory = sc_basic_authorship::ProposerFactory::with_proof_recording(
		task_manager.spawn_handle(),
		client.clone(),
		transaction_pool,
		prometheus_registry,
		telemetry.clone(),
	);

	let proposer = Proposer::new(proposer_factory);

	let collator_service = CollatorService::new(
		client.clone(),
		Arc::new(task_manager.spawn_handle()),
		announce_block,
		client.clone(),
	);

	let params = nimbus_consensus::collators::basic::Params {
		para_id,
		overseer_handle,
		proposer,
		create_inherent_data_providers: move |_, _| async move { Ok(()) },
		block_import,
		relay_client: relay_chain_interface,
		para_client: client,
		keystore,
		collator_service,
		force_authoring,
		max_pov_percentage,
		additional_digests_provider: (),
		collator_key,
		//authoring_duration: Duration::from_millis(500),
	};

	let fut = nimbus_consensus::collators::basic::run::<Block, _, _, ParachainBackend, _, _, _, _, _>(
		params,
	);
	task_manager
		.spawn_essential_handle()
		.spawn("nimbus", None, fut);

	Ok(())
}

/// Start a parachain node.
#[allow(deprecated)]
pub async fn start_parachain_node(
	parachain_config: Configuration,
	polkadot_config: Configuration,
	collator_options: CollatorOptions,
	para_id: ParaId,
	max_pov_percentage: u8,
) -> sc_service::error::Result<(
	TaskManager,
	Arc<TFullClient<Block, RuntimeApi, NativeElseWasmExecutor<TemplateRuntimeExecutor>>>,
)> {
	start_node_impl::<sc_network::NetworkWorker<_, _>>(
		parachain_config,
		polkadot_config,
		collator_options,
		para_id,
		max_pov_percentage,
	)
	.await
}

use sc_transaction_pool_api::OffchainTransactionPoolFactory;
/// Builds a new service for a full client.
pub fn start_instant_seal_node<N>(config: Configuration) -> Result<TaskManager, sc_service::Error>
where
	N: sc_network::NetworkBackend<Block, <Block as BlockT>::Hash>,
{
	let sc_service::PartialComponents {
		client,
		backend,
		mut task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		other: (_, mut telemetry, _),
	} = new_partial(&config)?;

	let prometheus_registry = config.prometheus_registry().cloned();
	let net_config =
		FullNetworkConfiguration::<_, _, N>::new(&config.network, prometheus_registry.clone());

	let metrics = N::register_notification_metrics(
		config.prometheus_config.as_ref().map(|cfg| &cfg.registry),
	);

	let (network, system_rpc_tx, tx_handler_controller, sync_service) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			import_queue,
			block_announce_validator_builder: None,
			warp_sync_config: None,
			net_config,
			block_relay: None,
			metrics,
		})?;

	if config.offchain_worker.enabled {
		task_manager.spawn_handle().spawn(
			"offchain-workers-runner",
			"offchain-work",
			sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
				runtime_api_provider: client.clone(),
				keystore: Some(keystore_container.keystore()),
				offchain_db: backend.offchain_storage(),
				transaction_pool: Some(OffchainTransactionPoolFactory::new(
					transaction_pool.clone(),
				)),
				network_provider: Arc::new(network.clone()),
				is_validator: config.role.is_authority(),
				enable_http_requests: false,
				custom_extensions: move |_| vec![],
			})?
			.run(client.clone(), task_manager.spawn_handle())
			.boxed(),
		);
	}

	let is_authority = config.role.is_authority();
	let prometheus_registry = config.prometheus_registry().cloned();

	let rpc_extensions_builder = {
		let client = client.clone();
		let transaction_pool = transaction_pool.clone();

		Box::new(move |_| {
			let deps = crate::rpc::FullDeps {
				client: client.clone(),
				pool: transaction_pool.clone(),
			};

			crate::rpc::create_full(deps).map_err(Into::into)
		})
	};
	let para_id = crate::chain_spec::Extensions::try_get(&*config.chain_spec)
		.map(|e| e.para_id)
		.ok_or_else(|| "Could not find parachain ID in chain-spec.")?;

	sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		network,
		client: client.clone(),
		keystore: keystore_container.keystore(),
		task_manager: &mut task_manager,
		transaction_pool: transaction_pool.clone(),
		rpc_builder: rpc_extensions_builder,
		backend,
		system_rpc_tx,
		sync_service: sync_service.clone(),
		config,
		tx_handler_controller,
		telemetry: telemetry.as_mut(),
	})?;

	if is_authority {
		let proposer = sc_basic_authorship::ProposerFactory::new(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool.clone(),
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|t| t.handle()),
		);

		let client_set_aside_for_cidp = client.clone();

		// Create channels for mocked XCM messages.
		let (_downward_xcm_sender, downward_xcm_receiver) = flume::bounded::<Vec<u8>>(100);
		let (_hrmp_xcm_sender, hrmp_xcm_receiver) = flume::bounded::<(ParaId, Vec<u8>)>(100);

		let authorship_future = run_instant_seal(InstantSealParams {
			block_import: client.clone(),
			env: proposer,
			client: client.clone(),
			pool: transaction_pool.clone(),
			select_chain,
			consensus_data_provider: Some(Box::new(NimbusManualSealConsensusDataProvider {
				keystore: keystore_container.keystore(),
				client: client.clone(),
				additional_digests_provider: (),
				_phantom: Default::default(),
			})),
			create_inherent_data_providers: move |block, _extra_args| {
				let downward_xcm_receiver = downward_xcm_receiver.clone();
				let hrmp_xcm_receiver = hrmp_xcm_receiver.clone();

				let client_for_xcm = client_set_aside_for_cidp.clone();
				let current_para_head = client_set_aside_for_cidp
					.header(block)
					.expect("Header lookup should succeed")
					.expect("Header passed in as parent should be present in backend.");
				let should_send_go_ahead = match client_set_aside_for_cidp
					.runtime_api()
					.collect_collation_info(block, &current_para_head)
				{
					Ok(info) => info.new_validation_code.is_some(),
					Err(e) => {
						log::error!("Failed to collect collation info: {:?}", e);
						false
					}
				};

				async move {
					let time = sp_timestamp::InherentDataProvider::from_system_time();

					// The nimbus runtime is shared among all nodes including the parachain node.
					// Because this is not a parachain context, we need to mock the parachain inherent data provider.
					//TODO might need to go back and get the block number like how I do in Moonkit
					let mocked_parachain = MockValidationDataInherentDataProvider {
						additional_key_values: None,
						current_para_block: 0,
						current_para_block_head: None,
						relay_offset: 0,
						relay_blocks_per_para_block: 0,
						para_blocks_per_relay_epoch: 0,
						relay_randomness_config: (),
						xcm_config: MockXcmConfig::new(&*client_for_xcm, block, Default::default()),
						raw_downward_messages: downward_xcm_receiver.drain().collect(),
						raw_horizontal_messages: hrmp_xcm_receiver.drain().collect(),
						para_id: para_id.into(),
						upgrade_go_ahead: should_send_go_ahead.then(|| {
							log::info!(
								"Detected pending validation code, sending go-ahead signal."
							);
							UpgradeGoAhead::GoAhead
						}),
					};

					Ok((time, mocked_parachain))
				}
			},
		});

		task_manager.spawn_essential_handle().spawn_blocking(
			"instant-seal",
			None,
			authorship_future,
		);
	};

	Ok(task_manager)
}
